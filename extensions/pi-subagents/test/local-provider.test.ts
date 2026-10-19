import assert from "node:assert/strict";
import { describe, it } from "node:test";

import type { ChildSession, SessionFactoryOptions } from "../src/backends/local.ts";
import { LocalProvider } from "../src/backends/local.ts";
import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { SubagentManager } from "../src/manager.ts";
import { baseRequest } from "./helpers.ts";

/** The only fake in the suite: a scripted child session. */
class ScriptedSession implements ChildSession {
	prompts: string[] = [];
	aborts = 0;
	disposals = 0;
	sessionFile = "/tmp/child.jsonl";
	#answer = "";
	#fail: Error | undefined;
	#hold: boolean;
	#pending: Array<{ resolve: () => void; reject: (error: Error) => void }> = [];

	constructor(options: { hold?: boolean } = {}) {
		this.#hold = options.hold ?? false;
	}

	async prompt(text: string): Promise<void> {
		this.prompts.push(text);
		if (this.#fail) throw this.#fail;
		if (!this.#hold) {
			this.#answer = `answer:${text}`;
			return;
		}
		// Hold the run open so a test can decide how it ends.
		await new Promise<void>((resolve, reject) => {
			this.#pending.push({ resolve, reject });
		});
	}

	/** Finish the oldest held prompt. */
	settle(answer: string): void {
		this.#answer = answer;
		this.#pending.shift()?.resolve();
	}

	/** Fail the oldest held prompt. */
	crash(error: Error): void {
		this.#pending.shift()?.reject(error);
	}

	lastAssistantText(): string | undefined {
		return this.#answer;
	}

	async abort(): Promise<void> {
		this.aborts += 1;
	}

	dispose(): void {
		this.disposals += 1;
	}

	failWith(error: Error): void {
		this.#fail = error;
	}
}

const setup = (
	options: {
		inheritsParentContext?: boolean;
		factoryThrows?: boolean;
		sessions?: ScriptedSession[];
		hold?: boolean;
	} = {},
) => {
	const sessions = options.sessions ?? [];
	const seen: SessionFactoryOptions[] = [];
	const provider = new LocalProvider({
		name: "spawn",
		inheritsParentContext: options.inheritsParentContext ?? false,
		factory: async (factoryOptions) => {
			seen.push(factoryOptions);
			if (options.factoryThrows) throw new Error("factory exploded");
			const session = new ScriptedSession({ hold: options.hold });
			sessions.push(session);
			return session;
		},
	});
	const manager = new SubagentManager();
	manager.registry.register(provider);
	return { manager, provider, sessions, seen };
};

describe("LocalProvider", () => {
	it("declares its capabilities", () => {
		const { provider } = setup();
		assert.equal(provider.capabilities.continuable, true);
		assert.equal(provider.capabilities.inheritsParentContext, false);
		assert.equal(provider.capabilities.depthLimit, 1);
	});

	it("returns the child's final answer as the result", async () => {
		const { manager, sessions } = setup();
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		const result = await activation.wait();
		assert.equal(result.text, "answer:do the thing");
		assert.equal(result.stopReason, "completed");
		assert.deepEqual(sessions[0].prompts, ["do the thing"]);
		assert.equal(sessions[0].disposals, 1);
	});

	it("does not await the whole run before calling the child accepted", async () => {
		const { manager, sessions } = setup({ hold: true });
		// start() must resolve while the run is still in flight.
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		assert.equal(activation.childId, "sub-1");
		assert.deepEqual(sessions[0].prompts, ["do the thing"]);
		assert.equal(sessions[0].disposals, 0);
		sessions[0].settle("finished");
		const result = await activation.wait();
		assert.equal(result.text, "finished");
	});

	it("forwards further input to a continuable child", async () => {
		const { manager, sessions } = setup();
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		await activation.send("and also this");
		await activation.wait();
		assert.deepEqual(sessions[0].prompts, ["do the thing", "and also this"]);
	});

	it("interrupts through the session", async () => {
		const { manager, sessions } = setup();
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		await activation.interrupt();
		assert.equal(sessions[0].aborts, 1);
		await activation.wait();
	});

	it("reports a start failure with a stable code", async () => {
		const { manager } = setup({ factoryThrows: true });
		const error = await manager.start(baseRequest({ provider: "spawn" })).catch((e: unknown) => e);
		assert.equal((error as SubagentError).code, SubagentErrorCode.START_REJECTED);
	});

	it("reports a child failure as an error result instead of throwing from wait", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		sessions[0].crash(new Error("model exploded"));
		const result = await activation.wait();
		assert.equal(result.stopReason, "error");
		assert.equal(result.errorCode, SubagentErrorCode.START_REJECTED);
	});

	it("surfaces an immediate prompt rejection as a start failure", async () => {
		const manager = new SubagentManager();
		manager.registry.register(
			new LocalProvider({
				name: "spawn",
				inheritsParentContext: false,
				factory: async () => {
					const session = new ScriptedSession();
					session.failWith(new Error("rejected immediately"));
					return session;
				},
			}),
		);
		const error = await manager.start(baseRequest({ provider: "spawn" })).catch((e: unknown) => e);
		assert.equal((error as SubagentError).code, SubagentErrorCode.START_REJECTED);
		assert.equal(manager.childrenOf("parent-a").length, 0);
	});
});
