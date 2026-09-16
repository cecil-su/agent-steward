import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { StringEnum } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const TOOL_NAME = "steward_task";
const TASKCTL_TIMEOUT_MS = 30_000;
const EXPECTED_SCHEMA_VERSION = 3;
const SESSION_PREFIX = "pi";

const TASK_STATUSES = ["backlog", "todo", "in_progress", "in_review", "blocked", "done", "cancelled"] as const;
type TaskStatus = typeof TASK_STATUSES[number];
type TaskAction = "create" | "show" | "claim" | "update" | "retitle" | "note" | "checkpoint" | "status";
type TaskReference = string | number;

type TaskView = {
	id: number;
	taskKey: string | null;
	title: string | null;
	status: TaskStatus;
	version: number;
	currentSessionId: string | null;
	nextStep: string | null;
};

type TaskctlError = {
	code: string;
	message: string;
	retryable: boolean;
	details: Record<string, unknown>;
};

type TaskctlEnvelope = {
	schemaVersion: number;
	ok: boolean;
	data: Record<string, unknown> | null;
	warnings: unknown[];
	error: TaskctlError | null;
};

type ToolParams = {
	action: TaskAction;
	taskId?: TaskReference;
	taskKey?: string;
	title?: string;
	goal?: string;
	scope?: string;
	acceptanceCriteria?: string;
	nextStep?: string | null;
	takeOver?: boolean;
	noteType?: "decision" | "progress" | "risk";
	text?: string;
	summary?: string;
	completed?: string[];
	decisions?: string[];
	pending?: string[];
	risks?: string[];
	status?: TaskStatus;
	expectedVersion?: number;
	reason?: string;
	confirmedByUser?: boolean;
};

type CommandResult = {
	envelope: TaskctlEnvelope;
	exitCode: number;
	stderr: string;
};

type ProcessResult = {
	stdout: string;
	stderr: string;
	code: number;
};

function requiredText(value: string | null | undefined, field: string): string {
	const text = value?.trim();
	if (!text) throw new Error(`${field} is required for this action`);
	return text;
}

function validatedTaskTitle(value: string | null | undefined): string {
	const title = requiredText(value, "title");
	const parts = title.split("｜");
	if (parts.length !== 3) throw new Error("title must use MMDD｜类型｜主题 with full-width separators");
	if (!/^[0-9]{4}$/.test(parts[0])) throw new Error("title MMDD must contain four digits");
	const month = Number(parts[0].slice(0, 2));
	const day = Number(parts[0].slice(2));
	const maximumDay = [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month] ?? 0;
	if (day < 1 || day > maximumDay) throw new Error("title MMDD must be a valid month and day");
	const taskTypes = ["功能", "设计", "修复", "优化", "发布", "探索", "文档", "研究"];
	if (!taskTypes.includes(parts[1])) throw new Error(`title type must be one of ${taskTypes.join("、")}`);
	if (!parts[2] || parts[2].trim() !== parts[2]) {
		throw new Error("title subject must be non-empty without surrounding whitespace");
	}
	return title;
}

function normalizeTaskReference(value: TaskReference): string {
	if (typeof value === "number") {
		if (!Number.isSafeInteger(value) || value <= 0) throw new Error("taskId must be a positive safe integer");
		return String(value);
	}
	const reference = value.trim();
	if (!reference) throw new Error("taskId must be a non-empty task reference");
	return reference;
}

function parseTask(value: unknown): TaskView | undefined {
	if (!value || typeof value !== "object") return undefined;
	const task = value as Partial<TaskView>;
	const statuses: readonly TaskStatus[] = TASK_STATUSES;
	if (
		typeof task.id !== "number" ||
		!Number.isSafeInteger(task.id) ||
		task.id <= 0 ||
		(task.taskKey !== null && typeof task.taskKey !== "string") ||
		(task.title !== null && typeof task.title !== "string") ||
		typeof task.status !== "string" ||
		!statuses.includes(task.status as TaskStatus) ||
		typeof task.version !== "number" ||
		!Number.isSafeInteger(task.version) || task.version < 1 ||
		(task.currentSessionId !== null && typeof task.currentSessionId !== "string") ||
		(task.nextStep !== null && typeof task.nextStep !== "string")
	) {
		return undefined;
	}
	return task as TaskView;
}

function taskFromEnvelope(envelope: TaskctlEnvelope): TaskView | undefined {
	return parseTask(envelope.data?.task);
}

function taskLabel(task: TaskView): string {
	return `#${task.id} ${task.title ?? "Untitled"}`;
}

function localSessionId(piSessionId: string, taskId: number): string {
	const taskHash = createHash("sha256").update(String(taskId)).digest("hex").slice(0, 12);
	return `${SESSION_PREFIX}-${piSessionId}-${taskHash}`;
}

function isOwnedSession(sessionId: string | null, piSessionId: string): boolean {
	return sessionId === piSessionId || Boolean(sessionId?.startsWith(`${SESSION_PREFIX}-${piSessionId}-`));
}

function databaseArgs(): string[] {
	const database = process.env.AGENT_STEWARD_DATABASE?.trim();
	return database ? ["--database", database] : [];
}

function commandName(): string {
	return process.env.AGENT_STEWARD_TASKCTL?.trim() || "taskctl";
}

function adapterError(code: string, message: string, details: Record<string, unknown>): TaskctlEnvelope {
	return {
		schemaVersion: EXPECTED_SCHEMA_VERSION,
		ok: false,
		data: null,
		warnings: [],
		error: { code, message, retryable: false, details },
	};
}

function parseEnvelope(stdout: string, stderr: string, exitCode: number): TaskctlEnvelope {
	try {
		const value = JSON.parse(stdout) as TaskctlEnvelope;
		if (!value || typeof value.ok !== "boolean" || typeof value.schemaVersion !== "number" || !Array.isArray(value.warnings) || (value.ok ? (value.error !== null || !value.data || typeof value.data !== "object") : (!value.error || typeof value.error.code !== "string"))) {
			throw new Error("invalid envelope shape");
		}
		if (value.schemaVersion !== EXPECTED_SCHEMA_VERSION) {
			return adapterError(
				"ADAPTER_UNSUPPORTED_SCHEMA",
				`taskctl schemaVersion ${value.schemaVersion} is incompatible with adapter schemaVersion ${EXPECTED_SCHEMA_VERSION}`,
				{ actualSchemaVersion: value.schemaVersion, expectedSchemaVersion: EXPECTED_SCHEMA_VERSION, exitCode },
			);
		}
        if (exitCode !== 0 && value.ok) return adapterError("ADAPTER_INVALID_OUTPUT", "taskctl reported success with a nonzero exit code", { exitCode });
		return value;
	} catch (error) {
		return adapterError(
			"ADAPTER_INVALID_OUTPUT",
			`taskctl returned invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
			{ exitCode, stderr: stderr.trim().slice(0, 2_000) },
		);
	}
}

function executeTaskctlProcess(
	command: string,
	args: string[],
	stdin: string | undefined,
	signal?: AbortSignal,
): Promise<ProcessResult> {
	return new Promise((resolve, reject) => {
		if (signal?.aborted) {
			reject(signal.reason ?? new Error("taskctl execution aborted"));
			return;
		}

		const child = spawn(command, args, {
			shell: false,
			stdio: ["pipe", "pipe", "pipe"],
			windowsHide: true,
		});
		const stdout: Buffer[] = [];
		const stderr: Buffer[] = [];
		let settled = false;

		const cleanup = () => {
			clearTimeout(timer);
			signal?.removeEventListener("abort", onAbort);
		};
		const fail = (error: unknown) => {
			if (settled) return;
			settled = true;
			cleanup();
			reject(error instanceof Error ? error : new Error(String(error)));
		};
		const onAbort = () => {
			child.kill();
			fail(signal?.reason ?? new Error("taskctl execution aborted"));
		};
		const timer = setTimeout(() => {
			child.kill();
			fail(new Error(`taskctl timed out after ${TASKCTL_TIMEOUT_MS}ms; outcome may be unknown, re-read before any new mutation`));
		}, TASKCTL_TIMEOUT_MS);

		signal?.addEventListener("abort", onAbort, { once: true });
        let outputBytes = 0;
        const collect = (target: Buffer[], chunk: Buffer) => {
            outputBytes += chunk.length;
            if (outputBytes > 4 * 1024 * 1024) {
                child.kill();
                fail(new Error("taskctl output exceeded 4 MiB; command outcome may be unknown, do not retry blindly"));
            } else if (!settled) target.push(chunk);
        };
		child.stdout.on("data", (chunk: Buffer) => collect(stdout, chunk));
		child.stderr.on("data", (chunk: Buffer) => collect(stderr, chunk));
		child.stdin.on("error", () => {
			// The process result reports early stdin closure; avoid an unhandled EPIPE.
		});
		child.once("error", fail);
		child.once("close", (code) => {
			if (settled) return;
			settled = true;
			cleanup();
			resolve({
				stdout: Buffer.concat(stdout).toString("utf8"),
				stderr: Buffer.concat(stderr).toString("utf8"),
				code: code ?? -1,
			});
		});
		child.stdin.end(stdin, "utf8");
	});
}

function resultText(action: TaskAction, result: CommandResult, refreshed?: TaskctlEnvelope): string {
	const task = taskFromEnvelope(result.envelope);
	if (result.envelope.ok) {
		return `${action} succeeded${task ? `: ${taskLabel(task)} status=${task.status} version=${task.version}` : ""}`;
	}

	const error = result.envelope.error;
	const refreshTask = refreshed ? taskFromEnvelope(refreshed) : undefined;
	const suffix = refreshTask
		? ` Current task was re-read at version ${refreshTask.version}; the failed mutation was not retried.`
		: "";
	const details = error?.details && Object.keys(error.details).length > 0
		? `\nerror.details: ${JSON.stringify(error.details)}`
		: "";
	return `${action} failed: ${error?.code ?? "UNKNOWN"} ${error?.message ?? "taskctl failed"}.${suffix}${details}`;
}

function toolResult(action: TaskAction, result: CommandResult, refreshed?: TaskctlEnvelope) {
    const output = `${resultText(action, result, refreshed)}\n${JSON.stringify({ envelope: result.envelope, refreshed })}`;
    const lines = output.split("\n");
    const shortened = lines.slice(0, 1999).join("\n");
    const bytes = Buffer.from(shortened, "utf8");
    const truncated = lines.length > 1999 || bytes.length > 49 * 1024;
    const text = bytes.subarray(0, 49 * 1024).toString("utf8") + (truncated ? "\n[Output truncated; use taskctl directly for the full record.]" : "");
    if (!result.envelope.ok) throw new Error(text);
    return { content: [{ type: "text" as const, text }], details: { result, refreshed, truncated } };
}

export function registerAgentSteward(pi: ExtensionAPI, executeProcess = executeTaskctlProcess) {
	const taskSnapshots = new Map<number, TaskView>();
	const taskAliases = new Map<string, number>();
	const mutationsInFlight = new Set<number>();
	let activeTaskId: number | undefined;

	async function runTaskctl(args: string[], input: unknown | undefined, signal?: AbortSignal): Promise<CommandResult> {
		const cliArgs = [...databaseArgs(), "--json"];
		let stdin: string | undefined;
		if (input !== undefined) {
			cliArgs.push("--input", "-");
			stdin = `${JSON.stringify(input)}\n`;
		}
		cliArgs.push(...args);

		const execution = await executeProcess(commandName(), cliArgs, stdin, signal);
		let envelope = parseEnvelope(execution.stdout, execution.stderr, execution.code);
        if (envelope.ok && envelope.data?.task !== undefined && !taskFromEnvelope(envelope)) {
            envelope = adapterError("ADAPTER_INVALID_OUTPUT", "taskctl returned an invalid task snapshot", {});
        }
		return { envelope, exitCode: execution.code, stderr: execution.stderr };
	}

	function rememberTask(task: TaskView, requestedReference?: string): TaskView {
		taskSnapshots.set(task.id, task);
		taskAliases.set(String(task.id), task.id);
		taskAliases.set(`#${task.id}`, task.id);
		if (task.taskKey) {
			taskAliases.set(`key:${task.taskKey}`, task.id);
			const numeric = task.taskKey.split("").every((character) => character >= "0" && character <= "9");
			const displayedNumeric = /^#[0-9]+$/.test(task.taskKey);
			if (!numeric && !displayedNumeric && !task.taskKey.startsWith("key:")) {
				taskAliases.set(task.taskKey, task.id);
			}
		}
		if (requestedReference) taskAliases.set(requestedReference, task.id);
		return task;
	}

	function remember(envelope: TaskctlEnvelope, requestedReference?: string): TaskView | undefined {
		const task = taskFromEnvelope(envelope);
		return task ? rememberTask(task, requestedReference) : undefined;
	}

	async function showTask(taskReference: TaskReference, signal?: AbortSignal): Promise<CommandResult> {
		const reference = normalizeTaskReference(taskReference);
		const result = await runTaskctl(["task", "show", reference], undefined, signal);
		remember(result.envelope, reference);
		return result;
	}

	async function knownTask(
		taskReference: TaskReference,
		signal?: AbortSignal,
	): Promise<{ task?: TaskView; result?: CommandResult }> {
		const reference = normalizeTaskReference(taskReference);
		const result = await showTask(reference, signal);
		return { task: taskFromEnvelope(result.envelope), result };
	}

	async function runMutation(
		action: TaskAction,
		taskReference: TaskReference,
		buildArgs: (version: number, taskId: number, task: TaskView) => string[],
		input: unknown | undefined,
		signal?: AbortSignal,
        expectedVersion?: number,
	): Promise<{ result: CommandResult; refreshed?: TaskctlEnvelope }> {
		const observed = await knownTask(taskReference, signal);
        if (observed.task && expectedVersion !== undefined && observed.task.version !== expectedVersion) {
            return { result: { exitCode: 4, stderr: "", envelope: adapterError("VERSION_CONFLICT", "User-confirmed version is stale; confirm the refreshed task and patch again", { expectedVersion, currentVersion: observed.task.version }) }, refreshed: observed.result?.envelope };
        }
		if (!observed.task) {
			return {
				result:
					observed.result ??
					({
						exitCode: 2,
						stderr: "",
						envelope: adapterError("NOT_FOUND", "Task was not found", {
							taskReference: normalizeTaskReference(taskReference),
						}),
					} satisfies CommandResult),
			};
		}

		const taskId = observed.task.id;
		if (mutationsInFlight.size > 0) throw new Error("Another Agent Steward mutation is already running; wait for its result");
		mutationsInFlight.add(taskId);
		try {
			const result = await runTaskctl(buildArgs(observed.task.version, taskId, observed.task), input, signal);
			const task = remember(result.envelope);
			if (task) {
				if (action === "claim") activeTaskId = task.id;
			}

			if (result.envelope.error?.code !== "VERSION_CONFLICT") return { result };

			const refreshed = await showTask(taskId, signal);
			return { result, refreshed: refreshed.envelope };
		} finally {
			mutationsInFlight.delete(taskId);
		}
	}

	pi.registerTool({
		name: TOOL_NAME,
		label: "Agent Steward Task",
		description:
			"Manage Steward data through envelope v3. Seven business states are user-controlled; claim only attaches a Session. Supports create/show/claim/update/retitle/note/checkpoint/status. Output is bounded to 50 KiB.",
		promptSnippet: "Read and mutate local Agent Steward tasks through the stable taskctl JSON contract",
		promptGuidelines: [
			"Use steward_task only when the user explicitly asks to create or manage an Agent Steward task, or when this Pi session is already attached to one.",
			"A task may be created before all descriptive fields are known; steward_task update requires a non-empty reason and confirmedByUser=true only after explicit user authorization for that task, observed version, and patch. Agent-inferred changes or general task execution permission are not confirmation.",
			"Every non-null title must use MMDD｜类型｜主题. Types are 功能、设计、修复、优化、发布、探索、文档、研究; use the Asia/Shanghai session date.",
			"Use steward_task retitle for explicitly confirmed title-only corrections; it must not change business status.",
			"Call one Agent Steward mutation at a time and wait for its result before issuing the next mutation.",
			"Use steward_task status only after the user explicitly selects a target business status for the observed task/version. Session ownership or business status never grants execution permission. Never infer status from tests, claim, checkpoint, or session end.",
			"If VERSION_CONFLICT is returned, use the refreshed snapshot to reconsider the action; never silently retry the failed mutation.",
		],
		parameters: Type.Object({
			action: StringEnum(["create", "show", "claim", "update", "retitle", "note", "checkpoint", "status"] as const),
			taskId: Type.Optional(
				Type.Union([
					Type.Integer({ minimum: 1 }),
					Type.String({ minLength: 1, maxLength: 160 }),
				]),
			),
			taskKey: Type.Optional(Type.String({ minLength: 1, maxLength: 160 })),
			title: Type.Optional(Type.String({ minLength: 1, maxLength: 300 })),
			goal: Type.Optional(Type.String({ minLength: 1, maxLength: 4_000 })),
			scope: Type.Optional(Type.String({ minLength: 1, maxLength: 4_000 })),
			acceptanceCriteria: Type.Optional(Type.String({ minLength: 1, maxLength: 4_000 })),
			nextStep: Type.Optional(
				Type.Union([Type.String({ minLength: 1, maxLength: 2_000 }), Type.Null()]),
			),
			takeOver: Type.Optional(Type.Boolean()),
			noteType: Type.Optional(StringEnum(["decision", "progress", "risk"] as const)),
			text: Type.Optional(Type.String({ minLength: 1, maxLength: 4_000 })),
			summary: Type.Optional(Type.String({ minLength: 1, maxLength: 8_000 })),
			completed: Type.Optional(Type.Array(Type.String({ minLength: 1, maxLength: 2_000 }), { maxItems: 100 })),
			decisions: Type.Optional(Type.Array(Type.String({ minLength: 1, maxLength: 2_000 }), { maxItems: 100 })),
			pending: Type.Optional(Type.Array(Type.String({ minLength: 1, maxLength: 2_000 }), { maxItems: 100 })),
			risks: Type.Optional(Type.Array(Type.String({ minLength: 1, maxLength: 2_000 }), { maxItems: 100 })),
			status: Type.Optional(StringEnum(TASK_STATUSES)),
			expectedVersion: Type.Optional(Type.Integer({ minimum: 1 })),
			reason: Type.Optional(Type.String({ minLength: 1, maxLength: 4_000, description: "Required for update. Explain why the task is being changed." })),
			confirmedByUser: Type.Optional(Type.Boolean({ description: "Required true for update, retitle, status, and claim. For update/retitle/status also supply expectedVersion from the user-confirmed snapshot. Never infer confirmation from general execution permission." })),
		}),
		async execute(_toolCallId, rawParams, signal, _onUpdate, ctx) {
			const params = rawParams as ToolParams;
            if (["update", "retitle", "status", "claim"].includes(params.action) && params.confirmedByUser !== true) {
                throw new Error(`${params.action} requires confirmedByUser=true after explicit user authorization`);
            }
            if (["update", "retitle", "status"].includes(params.action) && (!Number.isSafeInteger(params.expectedVersion) || params.expectedVersion! < 1)) {
                throw new Error(`${params.action} requires the user-confirmed expectedVersion`);
            }

			if (params.action === "create") {
				const legacyTaskKey = params.taskId === undefined ? undefined : normalizeTaskReference(params.taskId);
				const explicitTaskKey = params.taskKey === undefined ? undefined : requiredText(params.taskKey, "taskKey");
				if (legacyTaskKey && explicitTaskKey && legacyTaskKey !== explicitTaskKey) {
					throw new Error("taskId and taskKey must match when both are supplied for create");
				}
				const input: Record<string, string> = {};
				const taskKey = explicitTaskKey ?? legacyTaskKey;
				if (taskKey) input.taskKey = taskKey;
				if (params.title !== undefined) input.title = validatedTaskTitle(params.title);
				if (params.goal !== undefined) input.goal = requiredText(params.goal, "goal");
				if (params.scope !== undefined) input.scope = requiredText(params.scope, "scope");
				if (params.acceptanceCriteria !== undefined) {
					input.acceptanceCriteria = requiredText(params.acceptanceCriteria, "acceptanceCriteria");
				}
				if (params.nextStep !== undefined && params.nextStep !== null) {
					input.nextStep = requiredText(params.nextStep, "nextStep");
				}
				const result = await runTaskctl(
					["task", "create"],
					Object.keys(input).length > 0 ? input : undefined,
					signal,
				);
				remember(result.envelope);
                return toolResult(params.action, result);
			}

			const taskReference = params.taskId ?? activeTaskId;
			if (taskReference === undefined) {
				throw new Error("taskId is required because this Pi session has no active Agent Steward task");
			}

			if (params.action === "show") {
				const result = await showTask(taskReference, signal);
				const task = taskFromEnvelope(result.envelope);
				if (task && isOwnedSession(task.currentSessionId, ctx.sessionManager.getSessionId())) activeTaskId = task.id;
                else if (task && activeTaskId === task.id) activeTaskId = undefined;
                return toolResult(params.action, result);
			}

			if (params.action === "claim" && activeTaskId !== undefined) {
				const target = await knownTask(taskReference, signal);
				if (target.task && target.task.id !== activeTaskId) {
					throw new Error(`This Pi session is already attached to #${activeTaskId}; use a separate Pi session or explicitly end that Session first`);
				}
			}

			let checkpointSessionId: string | undefined;
			if (params.action === "checkpoint") {
				const observed = await knownTask(taskReference, signal);
				const piSessionId = ctx.sessionManager.getSessionId();
				if (!observed.task || !isOwnedSession(observed.task.currentSessionId, piSessionId)) {
					throw new Error(`Task ${normalizeTaskReference(taskReference)} is not claimed by this Pi session`);
				}
				checkpointSessionId = observed.task.currentSessionId ?? undefined;
			}

			let mutation: { result: CommandResult; refreshed?: TaskctlEnvelope };
			switch (params.action) {
				case "claim": {
					const piSessionId = ctx.sessionManager.getSessionId();
					mutation = await runMutation(
						params.action,
						taskReference,
						(version, taskId) => [
							"task",
							"claim",
							String(taskId),
							"--session",
							localSessionId(piSessionId, taskId),
							"--if-version",
							String(version),
							...(params.takeOver ? ["--take-over"] : []),
						],
						undefined,
						signal,
					);
					break;
				}
				case "update": {
					if (params.confirmedByUser !== true) {
						throw new Error("update requires confirmedByUser=true after explicit user authorization for this task, observed version, and patch");
					}
					const reason = requiredText(params.reason, "reason");
					const patch: Record<string, string | null> = {};
					if (params.taskKey !== undefined) patch.taskKey = requiredText(params.taskKey, "taskKey");
					if (params.title !== undefined) patch.title = validatedTaskTitle(params.title);
					if (params.goal !== undefined) patch.goal = requiredText(params.goal, "goal");
					if (params.scope !== undefined) patch.scope = requiredText(params.scope, "scope");
					if (params.acceptanceCriteria !== undefined) {
						patch.acceptanceCriteria = requiredText(params.acceptanceCriteria, "acceptanceCriteria");
					}
					if (Object.prototype.hasOwnProperty.call(params, "nextStep")) {
						patch.nextStep = params.nextStep === null ? null : requiredText(params.nextStep, "nextStep");
					}
					if (Object.keys(patch).length === 0) throw new Error("update requires at least one patch field");
					mutation = await runMutation(
						params.action,
						taskReference,
						(version, taskId) => [
							"task",
							"update",
							"--yes",
							String(taskId),
							"--if-version",
							String(version),
							"--reason",
							reason,
						],
						patch,
						signal,
                        params.expectedVersion,
					);
					break;
				}
				case "retitle": {
					const title = validatedTaskTitle(params.title);
					mutation = await runMutation(
						params.action,
						taskReference,
						(version, taskId) => [
							"task",
							"retitle",
							String(taskId),
							"--if-version",
							String(version),
							"--title",
							title,
						],
						undefined,
						signal,
                        params.expectedVersion,
					);
					break;
				}
				case "note": {
					const noteType = params.noteType;
					if (!noteType) throw new Error("noteType is required for note");
					const text = requiredText(params.text, "text");
					mutation = await runMutation(
						params.action,
						taskReference,
						(version, taskId) => [
							"task",
							"note",
							String(taskId),
							"--if-version",
							String(version),
							"--type",
							noteType,
							"--text",
							text,
						],
						undefined,
						signal,
					);
					break;
				}
				case "checkpoint": {
					const summary = requiredText(params.summary, "summary");
					const nextStep = requiredText(params.nextStep, "nextStep");
					const input = {
						summary,
						completed: params.completed ?? [],
						decisions: params.decisions ?? [],
						pending: params.pending ?? [],
						nextStep,
						risks: params.risks ?? [],
					};
					if (!checkpointSessionId) throw new Error("checkpoint requires an active Agent Steward session");
					mutation = await runMutation(
						params.action,
						taskReference,
						(version, taskId) => [
							"task",
							"checkpoint",
							String(taskId),
							"--session",
							checkpointSessionId,
							"--if-version",
							String(version),
						],
						input,
						signal,
					);
					break;
				}
                case "status": {
                    if (!params.status || !TASK_STATUSES.includes(params.status)) throw new Error("status requires a supported target business status");
                    mutation = await runMutation(params.action, taskReference,
                        (version, taskId) => ["task", "status", String(taskId), params.status!, "--if-version", String(version)],
                        undefined, signal, params.expectedVersion);
                    break;
                }
				default:
					throw new Error(`Unsupported action: ${params.action}`);
			}

            return toolResult(params.action, mutation.result, mutation.refreshed);
		},
	});

    pi.on("session_start", async () => {
        taskSnapshots.clear();
        taskAliases.clear();
        mutationsInFlight.clear();
        activeTaskId = undefined;
        // No automatic discovery, claim, recovery, or database access in ordinary sessions.
    });

	pi.on("before_agent_start", async (event) => {
		if (activeTaskId === undefined) return;
		const task = taskSnapshots.get(activeTaskId);
		const context = task
			? `Current Agent Steward task: ${taskLabel(task)} (${task.status}, version ${task.version}). Next step: ${task.nextStep ?? "none"}.`
			: `Current Agent Steward task: #${activeTaskId}.`;
		return {
			systemPrompt: `${event.systemPrompt}\n\n${context}\nUse ${TOOL_NAME} for authoritative task changes. Business status and Session attachment do not grant execution permission. Only the user's explicit instructions authorize work or status changes.`,
		};
	});

    pi.registerCommand("steward-status", {
        description: "Read this session's explicitly selected Steward task (does not change business status)",
        handler: async (_args, ctx) => {
            const message = activeTaskId === undefined
                ? "No Agent Steward task is attached to this Pi session"
                : resultText("show", await showTask(activeTaskId));
            if (ctx.hasUI) ctx.ui.notify(message, "info");
        },
    });
}

export default function agentStewardExtension(pi: ExtensionAPI) {
    registerAgentSteward(pi);
}
