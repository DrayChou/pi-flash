/**
 * Profile documents: `~/.pi/agent/agents/<id>.md` (global) and `.pi/agents/<id>.md` (project).
 *
 * Frontmatter is deliberately a small YAML subset (scalars, `|` block scalars, and
 * `- item` lists). A profile that cannot be read fails closed with `PROFILE_INVALID`
 * rather than silently running with the parent's composition.
 */

import { readFile, readdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

import { SubagentError, SubagentErrorCode } from "./errors.ts";

/** Tool-permission tiers (034 recipe reuse). No `full+`: that stays delegator-only. */
export type ToolTier = "inherit" | "read-only" | "default" | "full";

export const TOOL_TIERS: readonly ToolTier[] = ["inherit", "read-only", "default", "full"];

/** Default turn budget when neither the delegation call nor the profile sets one. */
export const DEFAULT_MAX_TURNS = 100;

/**
 * Whitelist each tier resolves to before `createAgentSession`.
 *
 * `inherit` is handled separately (no allowlist): the child keeps pi's default
 * selection plus the codemode/tool_search overrides, i.e. the delegator's own
 * assembly. The three concrete tiers must name every tool explicitly because a
 * plain-name allowlist drops unlisted extension tools — `codemode`/`tool_search`
 * are therefore folded in for the tiers that keep them.
 */
export const TOOL_TIER_WHITELISTS: Record<Exclude<ToolTier, "inherit">, readonly string[]> = {
	// Read-only exploration: no scripting engine, no mutation, no MCP surface.
	"read-only": ["read", "grep", "find", "ls"],
	// Default coding set plus the codemode/tool_search infrastructure (M2.5).
	default: ["read", "bash", "edit", "write", "codemode", "tool_search"],
	// Every built-in tool; `agent` is absent by construction (children cannot delegate).
	full: ["bash", "read", "edit", "write", "grep", "find", "ls", "codemode", "tool_search"],
};

export interface Profile {
	id: string;
	description?: string;
	model?: string;
	/** Replacement persona; wins over the body when both are present. */
	systemPrompt?: string;
	/** Append persona — the document body. */
	appendSystemPrompt?: string;
	/** Tool tier; `undefined` means inherit the delegator's assembly. */
	tools?: ToolTier;
	/**
	 * Turn budget (soft limit). `undefined` means {@link DEFAULT_MAX_TURNS};
	 * `0` is the "unlimited" tier and disables the budget entirely.
	 */
	maxTurns?: number;
	/** Whether the project AGENTS.md loads into the child; `undefined` means true. */
	injectAgentsMd?: boolean;
	/** Character budget for inherited parent context. */
	inheritContextLimit?: number;
	/** `worktree` isolates the child in a separate checkout. */
	isolation?: "worktree";
	/** Where the profile came from, for diagnostics. */
	source: string;
}

export interface ProfilePaths {
	globalDir: string;
	projectDir: string;
}

export function defaultProfilePaths(cwd: string): ProfilePaths {
	return {
		globalDir: join(homedir(), ".pi", "agent", "agents"),
		projectDir: join(cwd, ".pi", "agents"),
	};
}

interface Frontmatter {
	data: Record<string, unknown>;
	body: string;
}

/** Minimal frontmatter reader: scalars, `|` block scalars, quoted scalars, and `- item` lists. */
export function parseFrontmatter(text: string, source = "<memory>"): Frontmatter {
	const normalized = text.replace(/\r\n/g, "\n");
	if (!normalized.startsWith("---\n")) {
		return { data: {}, body: normalized.trim() };
	}
	const end = normalized.indexOf("\n---", 4);
	if (end === -1) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: unterminated frontmatter`);
	}
	const block = normalized.slice(4, end);
	const body = normalized.slice(end + 4).replace(/^\n/, "").trim();
	const data: Record<string, unknown> = {};
	let listKey: string | undefined;
	const lines = block.split("\n");
	for (let i = 0; i < lines.length; i += 1) {
		const rawLine = lines[i];
		if (!rawLine.trim() || rawLine.trim().startsWith("#")) continue;
		const listItem = rawLine.match(/^\s*-\s+(.*)$/);
		if (listItem && listKey) {
			const list = (data[listKey] as string[]) ?? [];
			list.push(unquote(listItem[1].trim()));
			data[listKey] = list;
			continue;
		}
		const pair = rawLine.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
		if (!pair) continue;
		const [, key, value] = pair;
		listKey = undefined;
		if (value === "|" || value === "|-" || value === "|+") {
			// Block scalar: consume every following indented line, then dedent.
			const collected: string[] = [];
			let indent: number | undefined;
			while (i + 1 < lines.length) {
				const next = lines[i + 1];
				if (next.trim() === "") {
					collected.push("");
					i += 1;
					continue;
				}
				const leading = next.length - next.trimStart().length;
				if (leading === 0) break;
				if (indent === undefined) indent = leading;
				collected.push(next.slice(Math.min(indent, leading)));
				i += 1;
			}
			data[key] = collected.join("\n").replace(/\n+$/, "");
			continue;
		}
		if (value === "") {
			listKey = key;
			data[key] = [];
			continue;
		}
		data[key] = unquote(value.trim());
	}
	return { data, body };
}

function unquote(value: string): string {
	if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
		return value.slice(1, -1);
	}
	return value;
}

export function profileFromDocument(id: string, text: string, source: string): Profile {
	const { data, body } = parseFrontmatter(text, source);
	const tools = data.tools;
	if (tools !== undefined && !TOOL_TIERS.includes(tools as ToolTier)) {
		throw new SubagentError(
			SubagentErrorCode.PROFILE_INVALID,
			`${source}: tools must be one of ${TOOL_TIERS.join("/")} (got ${String(tools)})`,
		);
	}
	const isolation = data.isolation;
	if (isolation !== undefined && isolation !== "worktree") {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: unsupported isolation ${String(isolation)}`);
	}
	const limit = data.inheritContextLimit;
	if (limit !== undefined && (!Number.isSafeInteger(Number(limit)) || Number(limit) < 0)) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: inheritContextLimit must be a number`);
	}
	const maxTurns = data.maxTurns;
	// `0` is the "unlimited" tier (no budget); a positive integer is an explicit cap.
	if (maxTurns !== undefined && (!Number.isSafeInteger(Number(maxTurns)) || Number(maxTurns) < 0)) {
		throw new SubagentError(
			SubagentErrorCode.PROFILE_INVALID,
			`${source}: maxTurns must be a non-negative integer (0 = unlimited)`,
		);
	}
	const inject = data.injectAgentsMd;
	if (inject !== undefined && inject !== "true" && inject !== "false") {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: injectAgentsMd must be true or false`);
	}
	return {
		id,
		description: typeof data.description === "string" ? data.description : undefined,
		model: typeof data.model === "string" && data.model !== "" ? data.model : undefined,
		systemPrompt: typeof data.systemPrompt === "string" && data.systemPrompt !== "" ? data.systemPrompt : undefined,
		appendSystemPrompt: body === "" ? undefined : body,
		tools: tools as ToolTier | undefined,
		maxTurns: maxTurns === undefined ? undefined : Number(maxTurns),
		injectAgentsMd: inject === undefined ? undefined : inject === "true",
		inheritContextLimit: limit === undefined ? undefined : Number(limit),
		isolation: isolation as "worktree" | undefined,
		source,
	};
}

/** Load one profile by id; the project directory shadows the global one. */
export async function loadProfile(id: string, paths: ProfilePaths): Promise<Profile> {
	if (!/^[A-Za-z0-9._-]+$/.test(id)) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `invalid profile id: ${id}`);
	}
	for (const dir of [paths.projectDir, paths.globalDir]) {
		const file = join(dir, `${id}.md`);
		try {
			return profileFromDocument(id, await readFile(file, "utf8"), file);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") continue;
			throw asProfileError(error, file);
		}
	}
	throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `profile not found: ${id}`);
}

/** List profile ids from both directories; project entries win on collision. */
export async function listProfiles(paths: ProfilePaths): Promise<string[]> {
	const ids = new Set<string>();
	for (const dir of [paths.globalDir, paths.projectDir]) {
		let entries: string[];
		try {
			entries = await readdir(dir);
		} catch {
			continue;
		}
		for (const entry of entries) {
			if (entry.endsWith(".md")) ids.add(entry.slice(0, -3));
		}
	}
	return [...ids].sort();
}

function asProfileError(error: unknown, source: string): SubagentError {
	if (error instanceof SubagentError) return error;
	const message = error instanceof Error ? error.message : String(error);
	return new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: ${message}`, { cause: error });
}
