<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { formatBytes } from '$lib/format';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import DbTransferBanner from '$lib/components/DbTransferBanner.svelte';
	import type { DbTableInfo, DbTablePreview } from '$lib/types';
	import {
		ArrowLeft,
		Database,
		Table2,
		Search,
		RefreshCw,
		Download,
		Upload,
		Trash2,
		Columns3,
		Rows3,
		ChevronLeft,
		ChevronRight
	} from 'lucide-svelte';

	const serverId = $derived($page.url.searchParams.get('server') ?? 'local-mariadb');
	const dbName = $derived($page.url.searchParams.get('db') ?? '');
	const serverProfile = $derived(
		(wb.config.db_servers ?? []).find((s) => s.id === serverId) ?? null
	);

	let tables = $state<DbTableInfo[]>([]);
	let loadingTables = $state(false);
	let query = $state('');

	let selectedTable = $state<string | null>(null);
	let preview = $state<DbTablePreview | null>(null);
	let loadingPreview = $state(false);
	let previewTab = $state<'rows' | 'schema'>('rows');

	// Pagination state
	let pageSize = $state(50);
	let currentPage = $state(1);

	let busy = $state(false);
	let confirmDropOpen = $state(false);

	const filteredTables = $derived.by(() => {
		const q = query.trim().toLowerCase();
		if (!q) return tables;
		return tables.filter((t) => t.name.toLowerCase().includes(q));
	});

	const totalSizeBytes = $derived(tables.reduce((sum, t) => sum + t.size_bytes, 0));
	const totalRows = $derived(preview?.total_rows ?? 0);
	const totalPages = $derived(Math.max(1, Math.ceil(totalRows / pageSize)));
	const rangeStart = $derived(totalRows === 0 ? 0 : (currentPage - 1) * pageSize + 1);
	const rangeEnd = $derived(Math.min(totalRows, currentPage * pageSize));

	let tablesReqSeq = 0;
	let previewReqSeq = 0;

	async function loadTables() {
		if (!dbName) return;
		const seq = ++tablesReqSeq;
		loadingTables = true;
		try {
			const nextTables = await api.listDatabaseTables(serverId, dbName);
			if (seq !== tablesReqSeq) return;
			tables = nextTables;
			if (tables.length > 0 && (!selectedTable || !tables.some((t) => t.name === selectedTable))) {
				await selectTable(tables[0].name, 1);
			} else if (tables.length === 0) {
				selectedTable = null;
				preview = null;
			}
		} catch (e) {
			if (seq !== tablesReqSeq) return;
			wb.showToast(`Failed loading tables: ${e}`, 'error');
		} finally {
			if (seq === tablesReqSeq) {
				loadingTables = false;
			}
		}
	}

	async function selectTable(name: string, targetPage = 1) {
		const seq = ++previewReqSeq;
		selectedTable = name;
		currentPage = Math.max(1, targetPage);
		loadingPreview = true;
		try {
			const offset = (currentPage - 1) * pageSize;
			const nextPreview = await api.inspectTable(serverId, dbName, name, pageSize, offset);
			if (seq !== previewReqSeq) return;
			preview = nextPreview;
		} catch (e) {
			if (seq !== previewReqSeq) return;
			wb.showToast(`Failed inspecting table '${name}': ${e}`, 'error');
			preview = null;
		} finally {
			if (seq === previewReqSeq) {
				loadingPreview = false;
			}
		}
	}

	async function goToPage(nextPage: number) {
		if (!selectedTable) return;
		const clamped = Math.max(1, Math.min(totalPages, nextPage));
		if (clamped === currentPage) return;
		await selectTable(selectedTable, clamped);
	}

	async function changePageSize(nextSize: number) {
		pageSize = nextSize;
		if (selectedTable) {
			await selectTable(selectedTable, 1);
		}
	}

	$effect(() => {
		// Re-run whenever URL query params change (server or db)
		const _s = serverId;
		const _d = dbName;
		if (!_s || !_d) return;
		selectedTable = null;
		preview = null;
		void loadTables();
	});

	async function handleExport() {
		busy = true;
		try {
			const res = await api.exportDatabase(serverId, dbName);
			if (res) {
				const [path, bytes] = res;
				wb.showToast(`Exported ${dbName} (${formatBytes(bytes)}) to ${path}`, 'success');
			}
		} catch (e) {
			wb.showToast(`Export failed: ${e}`, 'error');
		} finally {
			busy = false;
		}
	}

	async function handleImport() {
		busy = true;
		try {
			const path = await api.importDatabase(serverId, dbName);
			if (path) {
				wb.showToast(`Imported SQL into ${dbName}`, 'success');
				await loadTables();
			}
		} catch (e) {
			wb.showToast(`Import failed: ${e}`, 'error');
		} finally {
			busy = false;
		}
	}

	async function handleDropDatabase() {
		busy = true;
		try {
			await api.dropDatabase(serverId, dbName);
			wb.showToast(`Dropped database '${dbName}'`, 'info');
			goto('/databases');
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			busy = false;
		}
	}
</script>

<div class="db-detail-shell">
	<!-- Sticky Top Header -->
	<div class="db-sticky-header" style="background-color: var(--bg-app);">
		<div class="mb-2">
			<a
				href="/databases"
				class="inline-flex items-center gap-1.5 font-mono text-[11px] font-medium transition hover:text-[var(--text-primary)]"
				style="color: var(--text-muted);"
			>
				<ArrowLeft class="h-3.5 w-3.5" />
				Back to Databases
			</a>
		</div>

		<header
			class="flex flex-wrap items-end justify-between gap-4 border-b pb-3.5"
			style="border-color: var(--border-subtle);"
		>
			<div class="min-w-0">
				<div class="flex items-center gap-2">
					<Database class="h-4 w-4" style="color: var(--accent);" />
					<span class="wb-kicker">{serverProfile?.name ?? serverId}</span>
				</div>
				<h1 class="mt-0.5 truncate font-mono text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
					{dbName || 'Database'}
				</h1>
				<div class="mt-0.5 font-mono text-[11px]" style="color: var(--text-muted);">
					{tables.length} {tables.length === 1 ? 'table' : 'tables'} · {formatBytes(totalSizeBytes)}
				</div>
			</div>

			<div class="flex flex-wrap items-center gap-2">
				<button type="button" disabled={loadingTables} onclick={loadTables} class="wb-btn">
					<RefreshCw class="h-3.5 w-3.5 {loadingTables ? 'animate-spin' : ''}" />
					Refresh
				</button>
				<button type="button" disabled={busy} onclick={handleImport} class="wb-btn">
					<Upload class="h-3.5 w-3.5" />
					Import SQL
				</button>
				<button type="button" disabled={busy} onclick={handleExport} class="wb-btn">
					<Download class="h-3.5 w-3.5" />
					Export SQL
				</button>
				<button type="button" disabled={busy} onclick={() => (confirmDropOpen = true)} class="wb-btn-danger">
					<Trash2 class="h-3.5 w-3.5" />
					Drop
				</button>
			</div>
		</header>
	</div>

	<DbTransferBanner />

	<div class="db-explorer-layout">
		<!-- Left: Tables list -->
		<section
			class="db-tables-pane border"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			<div class="db-pane-header space-y-2.5 p-3.5">
				<div class="flex items-center justify-between">
					<span class="wb-kicker">Tables ({filteredTables.length})</span>
				</div>

				<div class="relative">
					<Search
						class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
						style="color: var(--text-muted);"
					/>
					<input
						type="search"
						bind:value={query}
						placeholder="Filter tables..."
						class="w-full py-1.5 pl-9 pr-3 text-xs outline-none"
						style="color: var(--text-primary);"
					/>
				</div>
			</div>

			<div class="db-tables-scroll px-2.5 pb-3">
				{#if loadingTables && tables.length === 0}
					<div class="space-y-2 p-1">
						{#each Array(6) as _}
							<div class="h-9 animate-pulse rounded-[10px]" style="background-color: var(--bg-elevated);"></div>
						{/each}
					</div>
				{:else if filteredTables.length === 0}
					<div class="py-10 text-center font-mono text-xs" style="color: var(--text-muted);">
						NO TABLES FOUND
					</div>
				{:else}
					<div class="space-y-0.5">
						{#each filteredTables as tbl (tbl.name)}
							{@const active = selectedTable === tbl.name}
							<button
								type="button"
								onclick={() => selectTable(tbl.name, 1)}
								class="flex w-full items-center justify-between gap-2 rounded-[10px] px-3 py-2 text-left transition hover:bg-[var(--bg-surface-hover)]"
								style="background-color: {active ? 'var(--accent-soft)' : 'transparent'};"
							>
								<span class="flex items-center gap-2 min-w-0">
									<Table2
										class="h-3.5 w-3.5 shrink-0"
										style="color: {active ? 'var(--accent)' : 'var(--text-muted)'};"
									/>
									<span
										class="truncate font-mono text-xs font-medium"
										style="color: {active ? 'var(--accent)' : 'var(--text-primary)'};"
									>
										{tbl.name}
									</span>
								</span>
								<span class="shrink-0 font-mono text-[10.5px] tabular-nums" style="color: var(--text-muted);">
									{tbl.row_count.toLocaleString()}
								</span>
							</button>
						{/each}
					</div>
				{/if}
			</div>
		</section>

		<!-- Right: Table Schema & Paginated Data Preview -->
		<section
			class="db-preview-pane border"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			{#if !selectedTable}
				<div class="flex flex-1 items-center justify-center p-12 font-mono text-xs" style="color: var(--text-muted);">
					SELECT A TABLE TO INSPECT ITS ROWS AND SCHEMA
				</div>
			{:else}
				{@const activeMeta = tables.find((t) => t.name === selectedTable)}
				<div class="flex flex-wrap items-center justify-between gap-3 px-4 pt-4 pb-3">
					<div class="min-w-0">
						<div class="truncate font-mono text-sm font-semibold" style="color: var(--text-primary);">
							{selectedTable}
						</div>
						{#if activeMeta}
							<div class="font-mono text-[11px]" style="color: var(--text-muted);">
								{activeMeta.engine || activeMeta.table_type} · {totalRows.toLocaleString()} rows · {formatBytes(activeMeta.size_bytes)}
							</div>
						{/if}
					</div>
					<div class="flex items-center gap-1.5">
						<button
							type="button"
							onclick={() => (previewTab = 'rows')}
							class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium transition"
							style="
								background-color: {previewTab === 'rows' ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
								color: {previewTab === 'rows' ? 'var(--accent)' : 'var(--text-secondary)'};
							"
						>
							<Rows3 class="h-3.5 w-3.5" />
							Rows ({totalRows.toLocaleString()})
						</button>
						<button
							type="button"
							onclick={() => (previewTab = 'schema')}
							class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium transition"
							style="
								background-color: {previewTab === 'schema' ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
								color: {previewTab === 'schema' ? 'var(--accent)' : 'var(--text-secondary)'};
							"
						>
							<Columns3 class="h-3.5 w-3.5" />
							Schema ({preview?.columns.length ?? 0})
						</button>
					</div>
				</div>

				<div class="db-table-scroll mx-4 mb-3 rounded-[10px]" style="background-color: var(--bg-app);">
					{#if loadingPreview}
						<div class="py-16 text-center font-mono text-xs" style="color: var(--text-muted);">
							Loading {selectedTable}...
						</div>
					{:else if preview && previewTab === 'schema'}
						<table class="db-data-table w-full text-left font-mono text-xs">
							<thead>
								<tr style="color: var(--text-muted);">
									<th class="px-3.5 py-2.5 font-medium">Column</th>
									<th class="px-3.5 py-2.5 font-medium">Type</th>
									<th class="px-3.5 py-2.5 font-medium">Key</th>
									<th class="px-3.5 py-2.5 font-medium">Nullable</th>
									<th class="px-3.5 py-2.5 font-medium">Default</th>
								</tr>
							</thead>
							<tbody class="divide-y" style="border-color: var(--border-subtle);">
								{#each preview.columns as col (col.name)}
									<tr class="transition hover:bg-[var(--bg-surface-hover)]">
										<td class="px-3.5 py-2 font-semibold" style="color: var(--text-primary);">{col.name}</td>
										<td class="px-3.5 py-2" style="color: var(--accent);">{col.col_type}</td>
										<td class="px-3.5 py-2" style="color: var(--text-secondary);">{col.key || '—'}</td>
										<td class="px-3.5 py-2" style="color: var(--text-muted);">{col.nullable ? 'YES' : 'NO'}</td>
										<td class="px-3.5 py-2" style="color: var(--text-muted);">{col.default_value ?? 'NULL'}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{:else if preview && previewTab === 'rows'}
						{#if preview.rows.length === 0}
							<div class="py-16 text-center font-mono text-xs" style="color: var(--text-muted);">
								TABLE IS EMPTY (0 ROWS)
							</div>
						{:else}
							<table class="db-data-table w-full text-left font-mono text-xs">
								<thead>
									<tr style="color: var(--text-muted);">
										{#each preview.columns as col (col.name)}
											<th class="whitespace-nowrap px-3.5 py-2.5 font-medium">{col.name}</th>
										{/each}
									</tr>
								</thead>
								<tbody class="divide-y" style="border-color: var(--border-subtle);">
									{#each preview.rows as row}
										<tr class="transition hover:bg-[var(--bg-surface-hover)]">
											{#each row as cell}
												<td
													class="selectable max-w-64 truncate whitespace-nowrap px-3.5 py-2"
													style="color: {cell === 'NULL' ? 'var(--text-muted)' : 'var(--text-primary)'};"
													title={cell}
												>
													{cell}
												</td>
											{/each}
										</tr>
									{/each}
								</tbody>
							</table>
						{/if}
					{/if}
				</div>

				<!-- Pagination Footer -->
				{#if preview && previewTab === 'rows'}
					<div class="flex flex-wrap items-center justify-between gap-3 px-4 pb-3.5 text-xs">
						<div class="flex items-center gap-3 font-mono text-[11px]" style="color: var(--text-muted);">
							<span>
								Showing <strong style="color: var(--text-primary);">{rangeStart.toLocaleString()}–{rangeEnd.toLocaleString()}</strong> of
								<strong style="color: var(--text-primary);">{totalRows.toLocaleString()}</strong>
							</span>
							<select
								aria-label="Rows per page"
								value={pageSize}
								onchange={(e) => changePageSize(Number((e.currentTarget as HTMLSelectElement).value))}
								class="px-2.5 py-1 font-mono text-[11px]"
								style="min-height: 28px; color: var(--text-primary);"
							>
								<option value={25}>25 / page</option>
								<option value={50}>50 / page</option>
								<option value={100}>100 / page</option>
								<option value={200}>200 / page</option>
							</select>
						</div>

						<div class="flex items-center gap-1.5">
							<button
								type="button"
								disabled={loadingPreview || currentPage <= 1}
								onclick={() => goToPage(currentPage - 1)}
								class="wb-btn"
								style="min-height: 28px; padding: 4px 12px; font-size: 12px;"
							>
								<ChevronLeft class="h-3.5 w-3.5" />
								Prev
							</button>
							<span class="px-2 font-mono text-[11px] tabular-nums" style="color: var(--text-secondary);">
								{currentPage.toLocaleString()} / {totalPages.toLocaleString()}
							</span>
							<button
								type="button"
								disabled={loadingPreview || currentPage >= totalPages}
								onclick={() => goToPage(currentPage + 1)}
								class="wb-btn"
								style="min-height: 28px; padding: 4px 12px; font-size: 12px;"
							>
								Next
								<ChevronRight class="h-3.5 w-3.5" />
							</button>
						</div>
					</div>
				{/if}
			{/if}
		</section>
	</div>
</div>

<ConfirmDialog
	bind:open={confirmDropOpen}
	title="Drop database '{dbName}'?"
	confirmLabel="Drop Database"
	destructive={true}
	onConfirm={handleDropDatabase}
	onCancel={() => {}}
>
	<p>This permanently deletes all tables and data inside <strong>{dbName}</strong>.</p>
</ConfirmDialog>

<style>
	.db-detail-shell {
		display: flex;
		flex-direction: column;
		width: 100%;
		height: calc(100dvh - 56px);
		min-height: 0;
		gap: 16px;
	}

	.db-sticky-header {
		position: sticky;
		top: 0;
		z-index: 20;
		flex-shrink: 0;
		padding-top: 6px;
		box-shadow: 0 -20px 0 0 var(--bg-app);
	}

	.db-explorer-layout {
		display: flex;
		flex-direction: column;
		gap: 16px;
		flex: 1 1 0%;
		min-height: 0;
	}

	.db-tables-pane,
	.db-preview-pane {
		display: flex;
		flex-direction: column;
		width: 100%;
		min-width: 0;
		min-height: 0;
	}

	.db-tables-pane {
		max-height: 36dvh;
	}

	.db-preview-pane {
		flex: 1 1 0%;
	}

	.db-pane-header {
		flex-shrink: 0;
	}

	.db-tables-scroll {
		flex: 1 1 0%;
		min-height: 0;
		overflow-y: auto;
	}

	.db-table-scroll {
		flex: 1 1 0%;
		min-height: 0;
		overflow: auto;
	}

	.db-data-table {
		border-collapse: separate;
		border-spacing: 0;
	}

	.db-data-table thead th {
		position: sticky;
		top: 0;
		z-index: 10;
		background-color: var(--bg-elevated);
		border-bottom: 1px solid var(--border-subtle);
	}

	@media (min-width: 900px) {
		.db-explorer-layout {
			flex-direction: row;
			align-items: stretch;
		}

		.db-tables-pane {
			width: clamp(220px, 22dvw, 300px);
			max-height: none;
			flex-shrink: 0;
		}

		.db-preview-pane {
			flex: 1 1 0%;
			width: auto;
		}
	}
</style>
