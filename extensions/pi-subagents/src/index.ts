/**
 * pi-subagents — extension entry.
 *
 * Wires the dsh-subagent call mechanics (manager, registry, capacity, descriptor,
 * local providers) onto pi. Nothing here re-implements pi's own primitives:
 * the child is a real pi session, the result is a real tool result, and usage is
 * accounted by pi's nested-call bookkeeping.
 *
 * M1 scope: local `spawn` and `fork` providers, caller delivery, depth guard,
 * orphan reconciliation. Background delivery, codemode fan-out and worktrees
 * come later (see 045).
 */

import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { homedir } from "node:os";
import { join } from "node:path";

import type { ChildSession, SessionFactory, SessionFactoryOptions } from "./backends/local.ts";
import { LocalProvider } from "./backends/local.ts";
import { DESCRIPTOR_TYPE, parseDescriptor, reconcileOnStart, type SubagentDescriptor } from "./descriptor.ts";
import { SubagentError, SubagentErrorCode } from "./errors.ts";
import { SubagentManager } from "./manager.ts";
import { defaultProfilePaths, loadProfile } from "./profile.ts";
import { DELEGATION_PARAMETERS, delegationDescription, runDelegation } from "./tool.ts";

const DEFAULT_CAPACITY = 8;
const DEFAULT_MAX_DEPTH = 1;
const TOOL_NAME = "agent";
/// Must match `SUBAGENT_CHILD_MARKER` in `pi-link/src/sessions.rs`: the side-bar
/// filter keys on this custom entry, never on `parentSession`.
const CHILD_MARKER_TYPE = "subagent/child";

/** Session file -> delegation depth, so a child that reloads this extension still knows its depth. */
const childDepths = new Map<string, number>();

function numberFromEnv(name: string, fallback: number): number {
	const raw = process.env[name];
	if (!raw) return fallback;
	const value = Number(raw);
	return Number.isSafeInteger(value) && value > 0 ? value : fallback;
}

/**
 * Build a child-session factory over the pi SDK.
 *
 * The SDK is imported lazily: this module must stay loadable (and testable)
 * without the package resolving, and a missing SDK is reported as a stable code
 * instead of a module-resolution stack trace.
 */
function createSessionFactory(ctx: ExtensionContext, parentSessionFile: string | undefined): SessionFactory {
	return async (options: SessionFactoryOptions): Promise<ChildSession> => {
		const sdk = await importSdk();
		const model = await resolveModel(sdk, ctx, options.model);
		const parent = ctx.sessionManager as unknown as {
			getSessionDir?: () => string | undefined;
			getSessionFile?: () => string | undefined;
		};
		const parentFile = parentSessionFile ?? parent.getSessionFile?.();
		// Children live beside the parent so hosts can find and open them, and the
		// header records the parent link (045: parentSession).
		const sessionManager = sdk.SessionManager.create(options.cwd, parent.getSessionDir?.(), {
			parentSession: parentFile,
		});
		// Marker extension: the child stamps its own file with a `subagent/child`
		// custom entry at session_start. Session-file filtering (pi-link/sessions.rs)
		// keys on THIS marker, never on `parentSession` — pi-native forks carry that
		// header too and must keep their sidebar rows.
		// agentDir is mandatory for DefaultResourceLoader (resolvePath chokes on
		// undefined on Windows); reuse the parent's own agent dir.
		const loader = new sdk.DefaultResourceLoader({
			cwd: options.cwd,
			agentDir: sdk.getAgentDir(),
			// Custom loader → pi 不再套默认 agentDir；传 undefined 会在
			// resolvePath() 炸掉（undefined.startsWith）。
			agentDir: join(homedir(), ".pi", "agent"),
			extensionFactories: [
				{
					name: "subagent-child-marker",
					hidden: true,
					factory: (childPi: { on(event: string, handler: () => void): void; appendEntry(type: string, data?: unknown): void }) => {
						childPi.on("session_start", () => {
							try {
								childPi.appendEntry(CHILD_MARKER_TYPE, { provider: options.providerName, depth: options.depth });
							} catch {
								// Missing marker degrades to "child shows in the list"; never fatal.
							}
						});
					},
				},
			],
		});
		await loader.reload();
		const { session } = await sdk.createAgentSession({
			cwd: options.cwd,
			model,
			sessionManager,
			resourceLoader: loader,
			...(options.tools && options.tools.length > 0 ? { tools: options.tools } : {}),
		});
		// SDK sessions don't auto-bind the extension runtime: without this the
		// marker's session_start never fires and the entry is never written.
		await (session as unknown as { bindExtensions(bindings: unknown): Promise<void> }).bindExtensions({});
		const sessionFile = session.sessionFile;
		if (sessionFile) childDepths.set(sessionFile, currentDepthOf(ctx) + 1);
		return {
			prompt: (text: string) => session.prompt(text),
			lastAssistantText: () => session.getLastAssistantText(),
			abort: () => session.abort(),
			dispose: () => session.dispose(),
			sessionFile: sessionFile ?? parentFile,
		};
	};
}

interface SdkLike {
	createAgentSession(options: Record<string, unknown>): Promise<{ session: AgentSessionLike }>;
	getAgentDir(): string;
	SessionManager: {
		create(
			cwd: string,
			sessionDir?: string,
			options?: { id?: string; parentSession?: string },
		): unknown;
	};
	DefaultResourceLoader: new (options?: {
		cwd?: string;
		agentDir?: string;
		extensionFactories?: unknown[];
	}) => { reload(): Promise<void> };
}

interface AgentSessionLike {
	prompt(text: string): Promise<void>;
	getLastAssistantText(): string | undefined;
	abort(): Promise<void>;
	dispose(): void;
	sessionFile?: string;
}

async function importSdk(): Promise<SdkLike> {
	try {
		return (await import("@earendil-works/pi-coding-agent")) as unknown as SdkLike;
	} catch (error) {
		throw new SubagentError(
			SubagentErrorCode.BACKEND_UNSUPPORTED,
			"cannot load @earendil-works/pi-coding-agent to create a child session",
			{ cause: error },
		);
	}
}

async function resolveModel(sdk: SdkLike, ctx: ExtensionContext, requested?: string): Promise<unknown> {
	if (!requested) return ctx.model;
	const separator = requested.indexOf("/");
	if (separator <= 0) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `model must be provider/model, got ${requested}`);
	}
	const provider = requested.slice(0, separator);
	const id = requested.slice(separator + 1);
	const registry = ctx.modelRegistry as unknown as { find?: (provider: string, id: string) => unknown };
	const model = registry.find?.(provider, id);
	if (!model) {
		throw new SubagentError(SubagentErrorCode.BACKEND_UNSUPPORTED, `model not available: ${requested}`);
	}
	void sdk;
	return model;
}

function currentDepthOf(ctx: ExtensionContext): number {
	const file = (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile;
	if (!file) return 0;
	return childDepths.get(file) ?? 0;
}

export default function piSubagents(pi: ExtensionAPI): void {
	// Descriptors are persisted as custom entries, so a restart can tell which
	// children never settled. `persist` is rebound per session; the manager reads it
	// through this closure.
	let persist: (descriptor: SubagentDescriptor) => void = () => {};

	const manager = new SubagentManager({
		capacity: numberFromEnv("PI_SUBAGENTS_CAPACITY", DEFAULT_CAPACITY),
		maxDepth: numberFromEnv("PI_SUBAGENTS_MAX_DEPTH", DEFAULT_MAX_DEPTH),
		onDescriptor: (descriptor) => persist(descriptor),
	});

	const makeFactory = (ctx: ExtensionContext): SessionFactory =>
		createSessionFactory(ctx, (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile);

	const spawnProvider = (ctx: ExtensionContext) =>
		new LocalProvider({
			name: "spawn",
			factory: makeFactory(ctx),
			inheritsParentContext: false,
			resolvePrompt: (request) => resolveProfile(ctx, request.profile, request.model),
		});

	const forkProvider = (ctx: ExtensionContext) =>
		new LocalProvider({
			name: "fork",
			factory: makeFactory(ctx),
			inheritsParentContext: true,
			resolvePrompt: (request) => resolveProfile(ctx, request.profile, request.model),
		});

	pi.registerTool({
		name: TOOL_NAME,
		label: "Subagent",
		description: delegationDescription({ inheritsParentContext: false, defaultProfile: undefined }),
		promptSnippet: "Delegate a task to a child agent with its own session",
		parameters: DELEGATION_PARAMETERS as unknown as never,
		exposure: "direct",
		namespace: {
			name: "subagents",
			description: "Delegate work to child agents",
			instructions:
				"Each child runs in its own pi session. Ask for independent work in parallel by issuing several calls in one turn.",
		},
		async execute(
			toolCallId: string,
			params: Record<string, unknown>,
			_signal: AbortSignal | undefined,
			_onUpdate: unknown,
			ctx: ExtensionContext,
		) {
			const depth = currentDepthOf(ctx);
			const parentId = (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile ?? "draft";
			const result = await runDelegation(
				{
					manager,
					providerName: "spawn",
					inheritsParentContext: false,
				},
				{
					prompt: String(params.prompt ?? ""),
					profile: typeof params.profile === "string" ? params.profile : undefined,
					model: typeof params.model === "string" ? params.model : undefined,
					cwd: typeof params.cwd === "string" ? params.cwd : undefined,
					label: typeof params.label === "string" ? params.label : undefined,
					background: params.background === true,
				},
				{
					parentId,
					depth,
					cwd: ctx.cwd,
					signal: ctx.signal,
					parentToolCallId: toolCallId,
				},
			);
			return result as never;
		},
	});

	pi.on("session_start", async (_event, ctx) => {
		// Re-register providers for this session's context (the factory closes over it).
		manager.registry.unregister("spawn");
		manager.registry.unregister("fork");
		manager.registry.register(spawnProvider(ctx));
		manager.registry.register(forkProvider(ctx));
		persist = (descriptor) => {
			try {
				pi.appendEntry(DESCRIPTOR_TYPE, descriptor);
			} catch {
				// Persistence is best-effort: a closed session must not break delegation.
			}
		};

		const inFlight = readDescriptors(ctx);
		if (inFlight.length > 0) {
			const reconciled = reconcileOnStart(inFlight, { aborted: false });
			for (const descriptor of reconciled) persist(descriptor);
		}
	});

	pi.on("session_shutdown", async () => {
		await manager.shutdown("session shutdown");
	});
}

function readDescriptors(ctx: ExtensionContext): SubagentDescriptor[] {
	const manager = ctx.sessionManager as unknown as { getBranch?: () => unknown[] };
	const entries = manager.getBranch?.() ?? [];
	const out: SubagentDescriptor[] = [];
	for (const entry of entries) {
		const record = entry as { type?: string; customType?: string; data?: unknown };
		if (record.type !== "custom" || record.customType !== DESCRIPTOR_TYPE) continue;
		try {
			out.push(parseDescriptor(record.data));
		} catch {
			// A descriptor we cannot read is ignored, never treated as a live child.
		}
	}
	return out;
}

async function resolveProfile(
	ctx: ExtensionContext,
	profileId: string | undefined,
	model: string | undefined,
): Promise<{ appendSystemPrompt?: string; tools?: string[]; model?: string }> {
	if (!profileId) return { model };
	const profile = await loadProfile(profileId, defaultProfilePaths(ctx.cwd));
	return {
		appendSystemPrompt: profile.appendSystemPrompt,
		tools: profile.tools,
		model: model ?? profile.model,
	};
}
