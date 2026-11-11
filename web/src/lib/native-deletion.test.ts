import fs from "node:fs";
import { afterEach, expect, it, vi } from "vitest";

const source = fs.readFileSync(
	"../crates/server/web-legacy-readonly/app.js",
	"utf8",
);
const html = fs.readFileSync(
	"../crates/server/web-legacy-readonly/index.html",
	"utf8",
);
afterEach(() => {
	document.body.replaceChildren();
	window.onpopstate = null;
	window.history.replaceState(null, "", "/");
	vi.useRealTimers();
	vi.restoreAllMocks();
});
function fixture(admin = true, capability = true, pathname = "/") {
	window.history.replaceState(null, "", pathname);
	document.body.innerHTML = new DOMParser().parseFromString(
		html,
		"text/html",
	).body.innerHTML;
	Object.defineProperty(document, "scrollingElement", {
		configurable: true,
		value: document.documentElement,
	});
	for (const dialog of document.querySelectorAll("dialog")) {
		dialog.showModal = () => dialog.setAttribute("open", "");
		dialog.close = () => {
			dialog.removeAttribute("open");
			dialog.dispatchEvent(new Event("close"));
		};
	}
	let task = {
		id: 1,
		version: 2,
		title: "合成删除任务",
		status: "todo",
		currentSessionId: null,
	};
	const work = {
		formatVersion: 1,
		requirements: ["R1", "R2"].map((id) => ({
			id,
			title: id + " 标题",
			acceptanceCriteria: "目标",
			status: "todo",
			result: "",
			nextStep: "",
			evidence: [],
			updatedAt: "2026-11-11",
		})),
		confirmations: [
			{
				id: "Q1",
				requirementIds: ["R1"],
				question: "独占确认",
				recommendation: "A",
				impact: "影响",
				status: "open",
				answer: "",
				evidence: [],
			},
			{
				id: "Q2",
				requirementIds: ["R1", "R2"],
				question: "共享确认",
				recommendation: "B",
				impact: "影响",
				status: "open",
				answer: "",
				evidence: [],
			},
		],
	};
	let deleted = false,
		outcome = "ok",
		blocked = false;
	const good = (data: unknown) => ({
		ok: true,
		status: 200,
		json: async () => ({ ok: true, data }),
	});
	const fail = (code: string, status = 409) => ({
		ok: false,
		status,
		json: async () => ({
			ok: false,
			error: {
				code,
				message: "synthetic error",
				details: { currentVersion: 3 },
			},
		}),
	});
	const transport = vi.fn(async (path: string, options: RequestInit) => {
		if (options.method === "POST") {
			if (outcome === "conflict") return fail("VERSION_CONFLICT");
			if (outcome === "blocked") return fail("DELETE_BLOCKED");
			if (outcome === "failed") return fail("INVALID_INPUT", 400);
			if (path.endsWith("/task-delete")) deleted = true;
			else {
				const body = JSON.parse(String(options.body));
				work.requirements = work.requirements.filter(
					(r) => r.id !== body.requirementId,
				);
				work.confirmations = work.confirmations
					.map((c) => ({
						...c,
						requirementIds: c.requirementIds.filter(
							(id) => id !== body.requirementId,
						),
					}))
					.filter((c) => c.requirementIds.length > 0);
			}
			task = { ...task, version: task.version + 1 };
			if (outcome === "uncertain")
				throw new Error("synthetic response loss after commit");
			return good({ task, work });
		}
		const url = new URL(path, "http://fixture");
		if (path.endsWith("/requirements/R1/delete-preview"))
			return good({
				task,
				requirement: work.requirements[0],
				removedConfirmations: [work.confirmations[0]],
				updatedConfirmations: [
					{ id: "Q2", question: "共享确认", requirementIds: ["R2"] },
				],
			});
		if (path.endsWith("/delete-preview"))
			return good({
				task,
				blocked,
				openSessionIds: blocked ? ["unfinished-old"] : [],
				retainedCounts: { requirements: 2, history: 3 },
				sourceReferences: { projectIds: [1], ruleIds: [3] },
			});
		if (path.endsWith("/audit"))
			return good({
				task,
				deletion: { reason: "fixture deletion", deletedAt: "2026-11-11" },
				work,
				notes: [],
				checkpoints: [{ id: 'audit-cp-1', sessionId: 'audit-session', createdAt: '2026-11-11T00:00:00Z', summary: 'CP-SUMMARY', completed: ['CP-COMPLETED'], decisions: ['CP-DECISION'], pending: ['CP-PENDING'], risks: ['CP-RISK'], nextStep: 'CP-NEXT', gitHead: 'historical-head' }],
				sessions: [],
				history: [],
			});
		if (path.endsWith("/context"))
			return deleted && path.includes('/1/')
				? fail("TASK_DELETED", 410)
				: good({
						task: path.includes("/2/")
							? { ...task, id: 2, title: "邻近任务" }
							: task,
						work: structuredClone(work),
						checkpoint: null,
					});
		if (path.endsWith("/notes")) return good({ notes: [] });
		const status = url.searchParams.get("status");
		return good({
			tasks: deleted || (status && status !== task.status) ? [] : [task],
			hasMore: false,
			nextCursor: null,
		});
	});
	const end = source.lastIndexOf("  startUiUpdates();");
	const code =
		source.slice(0, end).replace("(() => {", "return (() => {") +
		`connected=true;canChangeStatus=${admin};canDeleteTask=${admin && capability};canDeleteRequirement=${admin && capability};return {selectTask,openDelete,openTaskAudit,loadList,api,scheduleLiveRefresh,changeLayout,resetConnection,projectPage(){projectPageVisible=true;document.getElementById('task-page').hidden=true;document.getElementById('project-page').hidden=false;}};})();`;
	const api = new Function("document", "fetch", "navigator", code)(
		document,
		transport,
		{},
	);
	return {
		...api,
		transport,
		writes: () => transport.mock.calls.filter(([, o]) => o.method === "POST"),
		outcome: (value: string) => {
			outcome = value;
		},
		block: () => {
			blocked = true;
		},
		archive: () => { deleted = true; },
		drift: () => {
			task = { ...task, version: 3 };
		},
	};
}
async function submit() {
	(document.querySelector("#field-reason") as HTMLTextAreaElement).value =
		"明确删除合成对象";
	(document.querySelector("#field-confirmed") as HTMLInputElement).checked =
		true;
	await (document.querySelector("#action-form") as HTMLFormElement).onsubmit!(
		new Event("submit") as SubmitEvent,
	);
}
it.each([
	[false, true],
	[true, false],
])(
	"readers and old-backend admins have no deletion controls or POST transport (%s/%s)",
	async (admin, capability) => {
		const f = fixture(admin, capability);
		await f.selectTask(1);
		expect(
			[...document.querySelectorAll("button")].some(
				(b) => b.textContent === "删除任务" || b.textContent === "删除需求",
			),
		).toBe(false);
		await f.openDelete();
		await f.openDelete("R1");
		await expect(f.api("/api/commands/task-delete", {})).rejects.toThrow(
			"当前界面只读",
		);
		await expect(
			f.api("/api/commands/task-requirement-delete", {}),
		).rejects.toThrow("当前界面只读");
		expect(f.writes()).toHaveLength(0);
	},
);
it("previews exact target, retention and source references; cancellation sends no write", async () => {
	const f = fixture();
	await f.selectTask(1);
	await f.openDelete();
	expect(document.querySelector("#action-description")).toHaveTextContent(
		"#1 · 合成删除任务 · 版本 2",
	);
	expect(document.querySelector("#action-description")).toHaveTextContent(
		"不是物理清除",
	);
	expect(document.querySelector("#action-description")).toHaveTextContent(
		"项目 ##1；规则 #3",
	);
	(document.querySelector("#cancel-bottom") as HTMLButtonElement).click();
	expect(document.querySelector("#action-dialog")).not.toHaveAttribute("open");
	expect(f.writes()).toHaveLength(0);
});
it("shows exclusive and shared confirmation effects, then removes only the selected requirement", async () => {
	const f = fixture();
	await f.selectTask(1);
	await f.openDelete("R1");
	expect(document.querySelector("#action-description")).toHaveTextContent(
		"Q1 · 独占确认",
	);
	expect(document.querySelector("#action-description")).toHaveTextContent(
		"Q2 · 共享确认（剩余 R2）",
	);
	await submit();
	expect(f.writes()).toHaveLength(1);
	const [path, options] = f.writes()[0];
	expect(path).toBe("/api/commands/task-requirement-delete");
	expect(JSON.parse(String(options.body))).toEqual({
		taskId: 1,
		expectedVersion: 2,
		requirementId: "R1",
		confirmed: true,
		reason: "明确删除合成对象",
	});
	expect(options.headers).toMatchObject({
		"X-Steward-CSRF": "1",
		"X-Steward-UI-Contract": "5",
	});
	expect(document.querySelector('[data-requirement-id="R1"]')).toBeNull();
	expect(document.querySelector('[data-requirement-id="R2"]')).not.toBeNull();
	expect(document.querySelector("#detail")).not.toHaveTextContent("独占确认");
	expect(document.querySelector("#detail")).toHaveTextContent("关联需求：R2");
});
it("uses the shared dashboard detail entry and removes the card/count on task deletion; audit has no write buttons", async () => {
	const f = fixture(true, true, "/dashboard");
	await f.loadList();
	await f.selectTask(1);
	expect(document.body).toHaveClass("dashboard-detail-open");
	await f.openDelete();
	await submit();
	expect(f.writes()).toHaveLength(1);
	expect(document.querySelector(".task-card")).toBeNull();
	expect(document.querySelector("#task-count")).toHaveTextContent("0");
	expect(document.body).not.toHaveClass("dashboard-detail-open");
	await f.openTaskAudit(1);
	expect(document.querySelector("#detail")).toHaveTextContent("只读审计");
	expect(
		[...document.querySelectorAll("#detail button")].some((b) =>
			b.textContent?.startsWith("删除"),
		),
	).toBe(false);
});
it.each(['/', '/dashboard'])('keeps deleted-task audit through manual, SSE/reconnect and return-to-tasks refresh in %s', async pathname => {
	vi.useFakeTimers();
	const f = fixture(true, true, pathname); f.archive(); await f.openTaskAudit(1);
	const checkpoint = document.querySelector('[data-detail-key="audit-checkpoint-audit-cp-1"]') as HTMLDetailsElement;
	checkpoint.open = true;
	(document.querySelector('[data-detail-key="audit-history"]') as HTMLDetailsElement).open = true;
	await (document.querySelector('#refresh') as HTMLButtonElement).onclick!(new MouseEvent('click') as PointerEvent);
	for (let invalidation = 0; invalidation < 2; invalidation++) {
		f.scheduleLiveRefresh(); await vi.advanceTimersByTimeAsync(250);
	}
	f.projectPage();
	await (document.querySelector('#back-tasks') as HTMLButtonElement).onclick!(new MouseEvent('click') as PointerEvent);
	expect(document.querySelector('#detail')).toHaveTextContent('只读审计');
	expect(document.querySelector('#detail')).not.toHaveTextContent('TASK_DELETED');
	expect(document.querySelector('[data-detail-key="audit-checkpoint-audit-cp-1"]')).toHaveAttribute('open');
	expect(document.querySelector('[data-detail-key="audit-history"]')).toHaveAttribute('open');
	expect(f.transport.mock.calls.filter(([path]: [string]) => path.endsWith('/context'))).toHaveLength(0);
	expect([...document.querySelectorAll('#detail button')].some(b => b.textContent?.startsWith('删除'))).toBe(false);
	expect(f.writes()).toHaveLength(0);
});
it.each(['/', '/dashboard'])('exposes complete retained checkpoint contents in audit in %s', async pathname => {
	const f = fixture(true, true, pathname); f.archive(); await f.openTaskAudit(1);
	const checkpoint = document.querySelector('[data-detail-key="audit-checkpoint-audit-cp-1"]') as HTMLDetailsElement;
	checkpoint.open = true;
	for (const text of ['audit-cp-1', 'audit-session', '2026-11-11T00:00:00Z', 'CP-SUMMARY', 'CP-COMPLETED', 'CP-DECISION', 'CP-PENDING', 'CP-RISK', 'CP-NEXT', 'historical-head']) expect(checkpoint).toHaveTextContent(text);
	expect(f.writes()).toHaveLength(0);
});
it('clears audit selection for normal task, layout change and connection reset', async () => {
	const f = fixture(); f.archive(); await f.openTaskAudit(1); await f.selectTask(2);
	await (document.querySelector('#refresh') as HTMLButtonElement).onclick!(new MouseEvent('click') as PointerEvent);
	expect(document.querySelector('#detail')).toHaveTextContent('邻近任务');
	expect(document.querySelector('#detail')).not.toHaveTextContent('只读审计');
	await f.openTaskAudit(1); await f.changeLayout('board'); const count = f.transport.mock.calls.length;
	await (document.querySelector('#refresh') as HTMLButtonElement).onclick!(new MouseEvent('click') as PointerEvent);
	expect(f.transport.mock.calls.slice(count).some(([path]: [string]) => path.endsWith('/audit'))).toBe(false);
	await f.openTaskAudit(1); f.resetConnection();
	expect(document.querySelector('#detail')).toBeEmptyDOMElement(); expect(document.querySelector('#workspace')).toHaveAttribute('hidden');
	expect(f.writes()).toHaveLength(0);
});
it.each([['/', 'manual'], ['/dashboard', 'manual'], ['/', 'SSE'], ['/dashboard', 'SSE']])('does not reopen audit after navigation during %s %s list refresh', async (pathname, refreshKind) => {
	vi.useFakeTimers(); const f = fixture(true, true, pathname); f.archive(); await f.openTaskAudit(1);
	const base = f.transport.getMockImplementation()!;
	let finish!: () => void; let started!: () => void;
	const gate = new Promise<void>(resolve => { finish = resolve; });
	const began = new Promise<void>(resolve => { started = resolve; });
	f.transport.mockImplementation(async (path: string, options: RequestInit) => { if (path.startsWith('/api/tasks?')) { started(); await gate; } return base(path, options); });
	let refreshing;
	if (refreshKind === 'manual') refreshing = (document.querySelector('#refresh') as HTMLButtonElement).onclick!(new MouseEvent('click') as PointerEvent);
	else { f.scheduleLiveRefresh(); refreshing = vi.advanceTimersByTimeAsync(250); }
	await began; f.projectPage(); finish(); await refreshing;
	expect(document.querySelector('#project-page')).not.toHaveAttribute('hidden');
	expect(document.querySelector('#task-page')).toHaveAttribute('hidden');
	expect(f.transport.mock.calls.filter(([path]: [string]) => path.endsWith('/audit'))).toHaveLength(1);
	await (document.querySelector('#back-tasks') as HTMLButtonElement).onclick!(new MouseEvent('click') as PointerEvent);
	expect(document.querySelector('#detail')).toHaveTextContent('只读审计');
	expect(document.querySelector('#project-page')).toHaveAttribute('hidden');
	expect(f.writes()).toHaveLength(0);
});
it('discards a late audit read after a different task is selected', async () => {
	const f = fixture(); const base = f.transport.getMockImplementation()!;
	let finish!: () => void; const gate = new Promise<void>(resolve => { finish = resolve; });
	f.transport.mockImplementation(async (path: string, options: RequestInit) => { if (path.endsWith('/audit')) await gate; return base(path, options); });
	const pending = f.openTaskAudit(1); await f.selectTask(2); finish(); await pending;
	expect(document.querySelector('#detail')).toHaveTextContent('邻近任务'); expect(document.querySelector('#detail')).not.toHaveTextContent('只读审计');
	expect(f.writes()).toHaveLength(0);
});
it("blocks task deletion for unfinished Sessions without closing or taking over any Session", async () => {
	const f = fixture();
	f.block();
	await f.selectTask(1);
	await f.openDelete();
	await submit();
	expect(document.querySelector("#action-error")).toHaveTextContent(
		"unfinished-old",
	);
	expect(document.querySelector("#submit-action")).toBeDisabled();
	expect(f.writes()).toHaveLength(0);
});
it.each(["conflict", "uncertain", "blocked", "failed"])(
	"holds a %s submission and requires a fresh read, never retries a destructive request",
	async (outcome) => {
		const f = fixture();
		await f.selectTask(1);
		await f.openDelete();
		f.outcome(outcome);
		await submit();
		await submit();
		expect(f.writes()).toHaveLength(1);
		expect(document.querySelector("#submit-action")).toBeDisabled();
		expect(document.querySelector("#action-error")?.textContent).not.toBe("");
		const reconcile = [
			...document.querySelectorAll("#conflict button"),
		][0] as HTMLButtonElement;
		await reconcile.onclick!(new MouseEvent("click") as PointerEvent);
		expect(f.writes()).toHaveLength(1);
		if (outcome === "uncertain") {
			expect(document.querySelector("#detail")).toHaveTextContent(
				"TASK_DELETED",
			);
			expect(document.querySelector("#detail")).toHaveTextContent(
				"查看已删除任务的只读审计",
			);
		}
	},
);
it("serializes duplicate submissions and does not let Cancel hide an in-flight write", async () => {
	const f = fixture();
	await f.selectTask(1);
	await f.openDelete();
	const base = f.transport.getMockImplementation()!;
	let finish!: () => void;
	const gate = new Promise<void>((resolve) => {
		finish = resolve;
	});
	f.transport.mockImplementation(async (path: string, options: RequestInit) => {
		if (options.method === "POST") await gate;
		return base(path, options);
	});
	const pending = submit();
	await submit();
	(document.querySelector("#cancel") as HTMLButtonElement).click();
	expect(document.querySelector("#action-dialog")).toHaveAttribute("open");
	expect(f.writes()).toHaveLength(1);
	finish();
	await pending;
	expect(f.writes()).toHaveLength(1);
});
it("does not adopt a newer preview version into an old delete intent", async () => {
	const f = fixture();
	await f.selectTask(1);
	f.drift();
	await f.openDelete();
	expect(document.querySelector("#action-dialog")).not.toHaveAttribute("open");
	expect(document.querySelector("#notice")).toHaveTextContent("版本已变化");
	expect(f.writes()).toHaveLength(0);
});
it("discards a late preview after navigating to a neighboring task", async () => {
	const f = fixture();
	await f.selectTask(1);
	const base = f.transport.getMockImplementation()!;
	let finish!: () => void;
	const gate = new Promise<void>((resolve) => {
		finish = resolve;
	});
	f.transport.mockImplementation(async (path: string, options: RequestInit) => {
		if (path.endsWith("/delete-preview")) await gate;
		return base(path, options);
	});
	const pending = f.openDelete();
	await f.selectTask(2);
	finish();
	await pending;
	expect(document.querySelector("#action-dialog")).not.toHaveAttribute("open");
	expect(document.querySelector("#detail")).toHaveTextContent("邻近任务");
	expect(f.writes()).toHaveLength(0);
});
it("holds a success response with the wrong identity as uncertain", async () => {
	const f = fixture();
	await f.selectTask(1);
	await f.openDelete();
	const base = f.transport.getMockImplementation()!;
	f.transport.mockImplementation(async (path: string, options: RequestInit) => {
		if (options.method === "POST") return { ok: true, status: 200, json: async () => ({ ok: true, data: { task: { id: 2, version: 3 } } }) };
		return base(path, options);
	});
	await submit();
	await submit();
	expect(f.writes()).toHaveLength(1);
	expect(document.querySelector("#action-error")).toHaveTextContent("删除结果未确认");
	expect(document.querySelector("#submit-action")).toBeDisabled();
	expect(document.querySelector("#detail")).toHaveTextContent("合成删除任务");
});
it("reports saved deletion separately from failed refresh instead of allowing resubmit", async () => {
	const f = fixture();
	await f.selectTask(1);
	await f.openDelete();
	const base = f.transport.getMockImplementation()!;
	let saved = false;
	f.transport.mockImplementation(async (path: string, options: RequestInit) => {
		if (saved && options.method === "GET")
			throw new Error("synthetic read failure");
		const result = await base(path, options);
		if (options.method === "POST") saved = true;
		return result;
	});
	await submit();
	expect(f.writes()).toHaveLength(1);
	expect(document.querySelector("#notice")).toHaveTextContent(
		"删除已成功，但刷新失败",
	);
	expect(document.querySelector("#action-dialog")).not.toHaveAttribute("open");
});
