/**
 * Profile documents: `~/.pi/agent/agents/<id>.md` (global) and `.pi/agents/<id>.md` (project).
 *
 * Frontmatter is deliberately a small YAML subset (scalars and string lists).
 * A profile that cannot be read fails closed with `PROFILE_INVALID` rather than
 * silently running with the parent's composition.
 */

import { readFile, readdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

import { SubagentError, SubagentErrorCode } from "./errors.ts";

export interface Profile {
	id: string;
	description?: string;
	model?: string;
	appendSystemPrompt?: string;
	/** Tool allowlist; `undefined` means the default tool set. */
	tools?: string[];
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

/** Minimal frontmatter reader: scalars, quoted scalars, and `- item` lists. */
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
	for (const rawLine of block.split("\n")) {
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
		if (value === "") {
			listKey = key;
			data[key] = [];
			continue;
		}
		listKey = undefined;
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
	if (tools !== undefined && (!Array.isArray(tools) || tools.some((tool) => typeof tool !== "string"))) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: tools must be a list of strings`);
	}
	const isolation = data.isolation;
	if (isolation !== undefined && isolation !== "worktree") {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: unsupported isolation ${String(isolation)}`);
	}
	const limit = data.inheritContextLimit;
	if (limit !== undefined && (!Number.isSafeInteger(Number(limit)) || Number(limit) < 0)) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: inheritContextLimit must be a number`);
	}
	return {
		id,
		description: typeof data.description === "string" ? data.description : undefined,
		model: typeof data.model === "string" && data.model !== "" ? data.model : undefined,
		appendSystemPrompt: body === "" ? undefined : body,
		tools: tools as string[] | undefined,
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
