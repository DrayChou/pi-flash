/**
 * The `agent` tool: one tool, two delivery modes.
 *
 * - `delivery: "caller"` (M1 default): the call awaits the child and returns its
 *   answer, so the result is an ordinary tool result — pi's nested-call accounting
 *   covers usage, compaction and cancellation without a side channel.
 * - `delivery: "parent"` (v2 background): returns the child id immediately and the
 *   completion notification carries the answer.
 *
 * The description is generated from the profile's context inheritance so the model
 * cannot claim to see a conversation the child never received (dsh-subagent does
 * the same with two phrasings).
 */

import { SubagentError, SubagentErrorCode } from "./errors.ts";
import type { SubagentManager } from "./manager.ts";
import type { Delivery, SubagentResult } from "./types.ts";

export interface DelegationToolOptions {
	manager: SubagentManager;
	providerName: string;
	/** Tool name; must be unique among loaded delegation tools. */
	name?: string;
	/** Default profile id when the model does not choose one. */
	defaultProfile?: string;
	/** Profiles the model may choose. Empty means it cannot override the default. */
	allowedProfiles?: string[];
	/** Whether the backend child starts from the parent's completed history. */
	inheritsParentContext: boolean;
}

export interface DelegationContext {
	parentId: string;
	/** Depth of the calling agent; the child is one deeper. */
	depth: number;
	cwd: string;
	signal?: AbortSignal;
	parentToolCallId?: string;
	/** Called whenever the child's descriptor changes, for `pi.appendEntry`. */
	onDescriptor?: (descriptor: unknown) => void;
}

export interface ToolResultLike {
	content: Array<{ type: "text"; text: string }>;
	structuredContent?: Record<string, unknown>;
	details?: unknown;
	isError?: boolean;
	/** Child-session token usage; pi surfaces it on the tool result (not main-context accounting). */
	usage?: Record<string, unknown>;
}

export const DELEGATION_PARAMETERS = {
	type: "object",
	additionalProperties: false,
	properties: {
		prompt: { type: "string", description: "The task for the child agent, stated in full." },
		profile: { type: "string", description: "Profile id selecting the child's model, prompt and tools." },
		model: { type: "string", description: "Exact model override, `provider/model`." },
		cwd: { type: "string", description: "Working directory for the child; defaults to the caller's." },
		label: { type: "string", description: "Short label shown in the transcript." },
		background: {
			type: "boolean",
			description: "Return immediately with a child id and report the answer when it completes.",
		},
	},
	required: ["prompt"],
} as const;

export function delegationDescription(options: Pick<DelegationToolOptions, "inheritsParentContext" | "allowedProfiles" | "defaultProfile">): string {
	const context = options.inheritsParentContext
		? "It does not see the current in-flight turn, only completed history."
		: "It does not see this conversation.";
	const profiles =
		options.allowedProfiles && options.allowedProfiles.length > 0
			? ` Choose a profile from: ${options.allowedProfiles.join(", ")}.`
			: options.defaultProfile
				? ` The profile defaults to ${options.defaultProfile}.`
				: "";
	return [
		"Delegate a task to a child agent that works in its own session.",
		context,
		"The child's final answer is returned as this tool's result.",
		`${profiles} Delegate independent work in parallel by issuing several calls in one turn.`,
	].join(" ");
}

export async function runDelegation(
	options: DelegationToolOptions,
	input: {
		prompt: string;
		profile?: string;
		model?: string;
		cwd?: string;
		label?: string;
		background?: boolean;
	},
	context: DelegationContext,
): Promise<ToolResultLike> {
	if (options.allowedProfiles && options.allowedProfiles.length > 0 && input.profile) {
		if (!options.allowedProfiles.includes(input.profile)) {
			return failure(SubagentErrorCode.PROFILE_INVALID, `profile not allowed: ${input.profile}`);
		}
	}
	const delivery: Delivery = input.background ? "parent" : "caller";
	try {
		const activation = await options.manager.start({
			provider: options.providerName,
			parentId: context.parentId,
			depth: context.depth + 1,
			prompt: input.prompt,
			profile: input.profile ?? options.defaultProfile,
			model: input.model,
			cwd: input.cwd ?? context.cwd,
			label: input.label,
			delivery,
			signal: context.signal,
		});
		if (delivery === "parent") {
			return {
				content: [{ type: "text", text: `started subagent ${activation.childId}` }],
				structuredContent: { childId: activation.childId, status: "started" },
				details: { childId: activation.childId, delivery },
			};
		}
		const result = await activation.wait();
		return success(result);
	} catch (error) {
		const failureError = SubagentError.is(error)
			? error
			: new SubagentError(SubagentErrorCode.START_REJECTED, error instanceof Error ? error.message : String(error));
		return failure(failureError.code, failureError.message);
	}
}

function success(result: SubagentResult): ToolResultLike {
	return {
		content: [{ type: "text", text: result.text }],
		structuredContent: {
			childId: result.childId,
			text: result.text,
			stopReason: result.stopReason,
			usage: result.usage,
			durationMs: result.durationMs,
		},
		details: { childId: result.childId, stopReason: result.stopReason },
		isError: result.stopReason === "error",
		...(result.usage ? { usage: result.usage as Record<string, unknown> } : {}),
	};
}

function failure(code: SubagentErrorCode, message: string): ToolResultLike {
	return {
		content: [{ type: "text", text: `${code}: ${message}` }],
		structuredContent: { errorCode: code, message },
		details: { errorCode: code },
		isError: true,
	};
}
