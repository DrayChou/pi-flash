/**
 * Local child sessions, expressed against a narrow interface.
 *
 * The backend does not import the pi SDK directly: it receives a `SessionFactory`.
 * That keeps the mechanics testable with a scripted session (the only fake in the
 * suite) and lets the extension entry decide how a real session is created.
 */

import type { ChildHandle, Provider, ProviderCapabilities, SubagentRequest, SubagentResult, SubagentUsage } from "../types.ts";
import { asSubagentError, SubagentError, SubagentErrorCode } from "../errors.ts";

/** The slice of an `AgentSession` a subagent needs. */
export interface ChildSession {
	/** Run one prompt; resolves when the run finishes. */
	prompt(text: string): Promise<void>;
	/** Latest assistant text, used as the child's answer. */
	lastAssistantText(): string | undefined;
	/** Token usage of the child's last run (assistant message usage). */
	usage?(): SubagentUsage | undefined;
	/** Stop the current run, keeping queued work. */
	abort(): Promise<void>;
	/** Release the session. Idempotent. */
	dispose(): void;
	/** Persisted session file, when the child is durable. */
	sessionFile?: string;
	/** Subscribe to session events for progress reporting. */
	subscribe?(listener: (event: unknown) => void): () => void;
}

export interface SessionFactoryOptions {
	cwd: string;
	model?: string;
	/** Prompt text prepended to the child's task, from the profile. */
	appendSystemPrompt?: string;
	/** Tool allowlist from the profile; undefined means the default tool set. */
	tools?: string[];
	/** Session file of the parent, recorded in the child header for fork children. */
	parentSession?: string;
	/** True when the child must start from the parent's completed history. */
	inheritParentContext?: boolean;
	/** Stamped into the child's marker entry (side-bar filtering keys on it). */
	providerName?: string;
	/** Delegation depth, recorded in the marker entry. */
	depth?: number;
}

export type SessionFactory = (options: SessionFactoryOptions) => Promise<ChildSession>;

export interface LocalProviderOptions {
	name: string;
	factory: SessionFactory;
	/** Whether children start from the parent's completed history. */
	inheritsParentContext: boolean;
	/** Numeric depth limit honoured by this backend. */
	depthLimit?: number;
	/** Whether children accept further messages. */
	continuable?: boolean;
	/** Resolve per-child composition (profile) before the session is created. */
	resolvePrompt?: (request: SubagentRequest) => Promise<{ appendSystemPrompt?: string; tools?: string[]; model?: string }>;
}

/** Child handle over one real session. */
class LocalChild implements ChildHandle {
	readonly id: string;
	#session: ChildSession;
	#run: Promise<void> | undefined;
	#failure: unknown;
	#disposed = false;
	#started_at: number | undefined;

	constructor(id: string, session: ChildSession) {
		this.id = id;
		this.#session = session;
	}

	get sessionFile(): string | undefined {
		return this.#session.sessionFile;
	}

	async accept(prompt: string): Promise<void> {
		// Admission means "the child took the task", not "the child finished it".
		this.#run = this.#session.prompt(prompt).catch((error: unknown) => {
			this.#failure = error;
		});
		// Let a synchronous rejection surface before we call the child accepted.
		await Promise.resolve();
		if (this.#failure) throw asSubagentError(this.#failure, SubagentErrorCode.START_REJECTED);
	}

	async send(message: string): Promise<void> {
		if (this.#disposed) throw new SubagentError(SubagentErrorCode.ADMISSION_CLOSED, "child disposed");
		this.#run = this.#session.prompt(message).catch((error: unknown) => {
			this.#failure = error;
		});
		await Promise.resolve();
		if (this.#failure) throw asSubagentError(this.#failure, SubagentErrorCode.START_REJECTED);
	}

	async interrupt(): Promise<void> {
		await this.#session.abort();
	}

	session_file(): string | undefined {
		return this.#session.sessionFile;
	}

	async result(): Promise<SubagentResult> {
		if (this.#run) await this.#run;
		const usage = this.#session.usage?.();
		const durationMs = this.#started_at !== undefined ? Date.now() - this.#started_at : undefined;
		if (this.#failure) {
			const failure = asSubagentError(this.#failure, SubagentErrorCode.START_REJECTED);
			return { childId: this.id, text: "", stopReason: "error", errorCode: failure.code, usage, durationMs };
		}
		return {
			childId: this.id,
			text: this.#session.lastAssistantText() ?? "",
			stopReason: "completed",
			usage,
			durationMs,
		};
	}

	async dispose(): Promise<void> {
		if (this.#disposed) return;
		this.#disposed = true;
		this.#session.dispose();
	}
}

/** Provider over local pi sessions: `spawn` (fresh) or `fork` (inherits completed history). */
export class LocalProvider implements Provider {
	readonly name: string;
	readonly capabilities: ProviderCapabilities;
	#options: LocalProviderOptions;
	readonly sessions = new Map<string, LocalChild>();

	constructor(options: LocalProviderOptions) {
		this.name = options.name;
		this.#options = options;
		this.capabilities = {
			continuable: options.continuable ?? true,
			inheritsParentContext: options.inheritsParentContext,
			depthLimit: options.depthLimit ?? 1,
			modelOptions: true,
			worktree: false,
		};
	}

	async prepare(request: SubagentRequest): Promise<ChildHandle> {
		const childId = request.childId;
		if (!childId) throw new SubagentError(SubagentErrorCode.START_REJECTED, "manager did not reserve a child id");
		const composition = this.#options.resolvePrompt ? await this.#options.resolvePrompt(request) : {};
		let session: ChildSession;
		try {
			session = await this.#options.factory({
				cwd: request.cwd ?? process.cwd(),
				model: composition.model ?? request.model,
				appendSystemPrompt: composition.appendSystemPrompt,
				tools: composition.tools,
				inheritParentContext: this.#options.inheritsParentContext,
				providerName: this.#options.name,
				depth: request.depth,
			});
		} catch (error) {
			throw asSubagentError(error, SubagentErrorCode.START_REJECTED);
		}
		const child = new LocalChild(childId, session);
		this.sessions.set(childId, child);
		return child;
	}
}
