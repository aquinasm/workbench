<script lang="ts">
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { formatBytes, formatIdleDays, kindBadgeClass, kindLabel } from '$lib/format';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import TechIcon from '$lib/components/TechIcon.svelte';
	import type { ProjectKind } from '$lib/types';
	import {
		Search,
		Snowflake,
		Play,
		Terminal,
		FolderOpen,
		ChevronDown,
		ChevronRight,
		Trash2,
		RefreshCw
	} from 'lucide-svelte';

	let activeTab = $state<'projects' | 'caches'>('projects');
	let query = $state('');
	let kindFilter = $state<'all' | ProjectKind>('all');
	let statusFilter = $state<'all' | 'active' | 'stale' | 'hibernated'>('all');
	let sortBy = $state<'size' | 'idle' | 'name'>('size');

	let selectedPaths = $state<Set<string>>(new Set());
	let expandedPaths = $state<Set<string>>(new Set());
	let bulkConfirmOpen = $state(false);
	let busyPath = $state<string | null>(null);

	// Global cache cleanup state
	const totalCacheBytes = $derived(wb.caches.reduce((sum, c) => sum + c.size_bytes, 0));
	let pendingCacheId = $state<string | null>(null);
	let pendingCacheName = $state('');
	let pendingCacheSize = $state(0);
	let cacheConfirmOpen = $state(false);
	let cleaningCacheId = $state<string | null>(null);

	function askCleanCache(id: string, name: string, size: number) {
		pendingCacheId = id;
		pendingCacheName = name;
		pendingCacheSize = size;
		cacheConfirmOpen = true;
	}

	async function doCleanCache() {
		if (!pendingCacheId) return;
		cleaningCacheId = pendingCacheId;
		await wb.cleanCache(pendingCacheId);
		cleaningCacheId = null;
		pendingCacheId = null;
	}

	const filtered = $derived.by(() => {
		const q = query.trim().toLowerCase();
		let list = wb.projects.filter((p) => {
			if (q && !p.name.toLowerCase().includes(q) && !p.path.toLowerCase().includes(q)) {
				return false;
			}
			if (kindFilter !== 'all' && p.kind !== kindFilter && !p.secondary_kinds.includes(kindFilter)) {
				return false;
			}
			if (statusFilter === 'active' && p.hibernated) return false;
			if (statusFilter === 'hibernated' && !p.hibernated) return false;
			if (statusFilter === 'stale' && (p.hibernated || p.idle_days < wb.config.stale_days)) {
				return false;
			}
			return true;
		});

		list = [...list].sort((a, b) => {
			if (sortBy === 'size') return b.total_reclaimable_bytes - a.total_reclaimable_bytes;
			if (sortBy === 'idle') return b.idle_days - a.idle_days;
			return a.name.localeCompare(b.name);
		});
		return list;
	});

	const selectedReclaimable = $derived(
		wb.projects
			.filter((p) => selectedPaths.has(p.path))
			.reduce((sum, p) => sum + p.total_reclaimable_bytes, 0)
	);

	function toggleSelect(path: string) {
		const next = new Set(selectedPaths);
		if (next.has(path)) next.delete(path);
		else next.add(path);
		selectedPaths = next;
	}

	function selectAllVisibleActive() {
		const next = new Set(selectedPaths);
		const activeVisible = filtered.filter((p) => !p.hibernated && p.total_reclaimable_bytes > 0);
		const allSelected = activeVisible.every((p) => next.has(p.path));
		if (allSelected) {
			for (const p of activeVisible) next.delete(p.path);
		} else {
			for (const p of activeVisible) next.add(p.path);
		}
		selectedPaths = next;
	}

	function toggleExpand(path: string) {
		const next = new Set(expandedPaths);
		if (next.has(path)) next.delete(path);
		else next.add(path);
		expandedPaths = next;
	}

	async function hibernateSingle(path: string, artifactRel?: string) {
		busyPath = path;
		await wb.hibernateOne(path, artifactRel ? [artifactRel] : undefined);
		busyPath = null;
	}

	async function runBulkHibernate() {
		const paths = Array.from(selectedPaths);
		selectedPaths = new Set();
		await wb.hibernateBulk(paths);
	}
</script>

<div class="mx-auto max-w-6xl space-y-5">
	<header class="flex flex-wrap items-end justify-between gap-3 border-b pb-4" style="border-color: var(--border-subtle);">
		<div>
			<h1 class="text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
				Storage
			</h1>
			<p class="mt-0.5 font-mono text-[11px]" style="color: var(--text-secondary);">
				Reclaimable: <strong>{formatBytes(wb.totalReclaimable + totalCacheBytes)}</strong>
				({formatBytes(wb.totalReclaimable)} projects · {formatBytes(totalCacheBytes)} caches)
			</p>
		</div>

		{#if activeTab === 'projects' && selectedPaths.size > 0}
			<button
				type="button"
				onclick={() => (bulkConfirmOpen = true)}
				class="wb-btn-primary"
			>
				<Snowflake class="h-3.5 w-3.5" />
				Hibernate {selectedPaths.size} Selected ({formatBytes(selectedReclaimable)})
			</button>
		{:else if activeTab === 'caches'}
			<button
				type="button"
				disabled={wb.scanningCaches}
				onclick={() => wb.refreshCaches()}
				class="wb-btn"
			>
				<RefreshCw class="h-3.5 w-3.5 {wb.scanningCaches ? 'animate-spin' : ''}" />
				Rescan Caches
			</button>
		{/if}
	</header>

	<!-- Segmented Tabs: Project Artifacts vs Global Caches -->
	<div
		class="inline-flex gap-1 rounded-xl border p-1"
		style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
	>
		<button
			type="button"
			onclick={() => (activeTab = 'projects')}
			class="inline-flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition"
			style="
				background-color: {activeTab === 'projects' ? 'var(--accent-soft)' : 'transparent'};
				color: {activeTab === 'projects' ? 'var(--accent)' : 'var(--text-secondary)'};
			"
		>
			<span>Project Artifacts</span>
			<span
				class="rounded-full px-1.5 py-0.5 text-[10px] font-semibold tabular-nums"
				style="background-color: var(--bg-app); color: var(--text-primary);"
			>
				{formatBytes(wb.totalReclaimable)}
			</span>
		</button>
		<button
			type="button"
			onclick={() => (activeTab = 'caches')}
			class="inline-flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition"
			style="
				background-color: {activeTab === 'caches' ? 'var(--accent-soft)' : 'transparent'};
				color: {activeTab === 'caches' ? 'var(--accent)' : 'var(--text-secondary)'};
			"
		>
			<span>Global Caches</span>
			<span
				class="rounded-full px-1.5 py-0.5 text-[10px] font-semibold tabular-nums"
				style="background-color: var(--bg-app); color: var(--text-primary);"
			>
				{formatBytes(totalCacheBytes)}
			</span>
		</button>
	</div>

	{#if activeTab === 'projects'}
	<!-- Filter Bar -->
	<div
		class="flex flex-wrap items-center gap-2.5 rounded-xl border p-3"
		style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
	>
		<div class="relative min-w-52 flex-1">
			<Search
				class="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
				style="color: var(--text-muted);"
			/>
			<input
				type="search"
				bind:value={query}
				placeholder="Filter by project name or path..."
				class="w-full rounded-lg border py-1.5 pl-8 pr-3 text-xs outline-none"
				style="background-color: var(--bg-app); border-color: var(--border-subtle); color: var(--text-primary);"
			/>
		</div>

		<select
			bind:value={kindFilter}
			aria-label="Filter by stack"
			class="rounded-lg border px-2.5 py-1.5 text-xs"
			style="background-color: var(--bg-app); border-color: var(--border-subtle); color: var(--text-primary);"
		>
			<option value="all">All Stacks</option>
			<option value="laravel">Laravel</option>
			<option value="sveltekit">SvelteKit</option>
			<option value="flutter">Flutter</option>
			<option value="rust">Rust</option>
			<option value="node">Node.js</option>
			<option value="php">PHP</option>
		</select>

		<select
			bind:value={statusFilter}
			aria-label="Filter by status"
			class="rounded-lg border px-2.5 py-1.5 text-xs"
			style="background-color: var(--bg-app); border-color: var(--border-subtle); color: var(--text-primary);"
		>
			<option value="all">All Status</option>
			<option value="active">Active (Has Artifacts)</option>
			<option value="stale">Stale (&ge;{wb.config.stale_days}d)</option>
			<option value="hibernated">Hibernated</option>
		</select>

		<select
			bind:value={sortBy}
			aria-label="Sort projects"
			class="rounded-lg border px-2.5 py-1.5 text-xs"
			style="background-color: var(--bg-app); border-color: var(--border-subtle); color: var(--text-primary);"
		>
			<option value="size">Sort: Largest First</option>
			<option value="idle">Sort: Most Idle</option>
			<option value="name">Sort: Name</option>
		</select>

		<button
			type="button"
			onclick={selectAllVisibleActive}
			class="rounded-lg border px-2.5 py-1.5 text-xs font-medium transition hover:opacity-80"
			style="border-color: var(--border-subtle); color: var(--text-secondary);"
		>
			Toggle Select Active
		</button>
	</div>

	<!-- Project List -->
	<div
		class="overflow-hidden rounded-xl border"
		style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
	>
		{#if filtered.length === 0}
			<div class="p-10 text-center text-xs" style="color: var(--text-secondary);">
				{wb.projects.length === 0
					? 'No projects scanned yet. Add a workspace folder from Dashboard or Settings.'
					: 'No projects match the current filter.'}
			</div>
		{:else}
			<div class="divide-y" style="border-color: var(--border-subtle);">
				{#each filtered as project (project.path)}
					{@const expanded = expandedPaths.has(project.path)}
					{@const isBusy = busyPath === project.path}
					<div>
						<div class="flex items-center gap-3 px-4 py-3">
							<input
								type="checkbox"
								disabled={project.hibernated}
								checked={selectedPaths.has(project.path)}
								onchange={() => toggleSelect(project.path)}
								aria-label="Select {project.name}"
								class="h-3.5 w-3.5 rounded"
							/>

							<button
								type="button"
								onclick={() => toggleExpand(project.path)}
								aria-label="Toggle artifact breakdown"
								class="rounded p-0.5 transition hover:opacity-75"
								style="color: var(--text-muted);"
							>
								{#if expanded}
									<ChevronDown class="h-4 w-4" />
								{:else}
									<ChevronRight class="h-4 w-4" />
								{/if}
							</button>

							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-center gap-2">
									<span style="color: {project.kind === 'laravel' || project.kind === 'php' ? 'var(--accent)' : 'var(--text-muted)'};">
										<TechIcon name={project.kind} class="h-3.5 w-3.5" />
									</span>
									<span class="truncate text-sm font-semibold" style="color: var(--text-primary);">
										{project.name}
									</span>
									<span
										class="rounded border px-1.5 py-0.5 text-[10px] font-semibold {kindBadgeClass(
											project.kind
										)}"
									>
										{kindLabel(project.kind)}
									</span>
									{#each project.secondary_kinds as sk}
										<span
											class="rounded border px-1.5 py-0.5 text-[10px] font-medium {kindBadgeClass(sk)}"
										>
											{kindLabel(sk)}
										</span>
									{/each}
									{#each Object.entries(project.version_hints) as [tool, ver]}
										<span
											class="rounded border px-1.5 py-0.5 font-mono text-[10px]"
											style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-secondary);"
										>
											{tool} {ver}
										</span>
									{/each}
									{#if project.hibernated}
										<span
											class="rounded-full px-2 py-0.5 text-[10px] font-medium"
											style="background-color: var(--bg-app); color: var(--text-muted);"
										>
											Hibernated
										</span>
									{:else if project.idle_days >= wb.config.stale_days}
										<span
											class="rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-medium text-amber-600 dark:text-amber-400"
										>
											{formatIdleDays(project.idle_days)}
										</span>
									{:else}
										<span class="text-[11px]" style="color: var(--text-muted);">
											{formatIdleDays(project.idle_days)}
										</span>
									{/if}
								</div>
								<div class="selectable mt-0.5 truncate font-mono text-[11px]" style="color: var(--text-muted);">
									{project.path}
								</div>
							</div>

							<div class="flex items-center gap-2">
								<span
									class="min-w-18 text-right text-sm font-semibold tabular-nums"
									style="color: {project.hibernated ? 'var(--text-muted)' : 'var(--text-primary)'};"
								>
									{formatBytes(project.total_reclaimable_bytes)}
								</span>

								{#if !project.hibernated}
									<button
										type="button"
										disabled={isBusy}
										onclick={() => hibernateSingle(project.path)}
										class="inline-flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs font-medium transition hover:opacity-85 disabled:opacity-40"
										style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
									>
										<Snowflake class="h-3 w-3" />
										{isBusy ? 'Cleaning...' : 'Hibernate'}
									</button>
								{:else if project.wake_commands.length > 0}
									<button
										type="button"
										disabled={wb.terminalRunning}
										onclick={() => wb.wakeOne(project.path)}
										class="inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs font-medium text-white transition hover:opacity-90 disabled:opacity-40"
										style="background-color: var(--accent);"
									>
										<Play class="h-3 w-3" />
										Wake
									</button>
								{/if}

								<button
									type="button"
									title="Open in Terminal"
									onclick={() => api.openInTerminal(project.path)}
									class="rounded-md p-1.5 transition hover:opacity-75"
									style="color: var(--text-secondary);"
								>
									<Terminal class="h-3.5 w-3.5" />
								</button>

								<button
									type="button"
									title="Reveal in File Manager"
									onclick={() => api.revealInFileManager(project.path)}
									class="rounded-md p-1.5 transition hover:opacity-75"
									style="color: var(--text-secondary);"
								>
									<FolderOpen class="h-3.5 w-3.5" />
								</button>
							</div>
						</div>

						{#if expanded}
							<div
								class="border-t px-12 py-3 text-xs space-y-2.5"
								style="border-color: var(--border-subtle); background-color: var(--bg-app);"
							>
								{#if project.artifacts.length === 0}
									<div style="color: var(--text-muted);">
										No regenerable artifacts on disk.
										{#if project.wake_commands.length > 0}
											Wake will run:
											<code class="font-mono">{project.wake_commands.join(' && ')}</code>
										{/if}
									</div>
								{:else}
									<div class="space-y-1.5">
										{#each project.artifacts as art}
											<div
												class="flex items-center justify-between rounded-lg border px-3 py-2"
												style="border-color: var(--border-subtle); background-color: var(--bg-surface);"
											>
												<div>
													<span class="font-mono font-medium" style="color: var(--text-primary);">
														{art.rel_path}
													</span>
													<span class="ml-2 text-[11px]" style="color: var(--text-muted);">
														{art.label}
													</span>
												</div>
												<div class="flex items-center gap-3">
													<span class="font-semibold tabular-nums" style="color: var(--text-primary);">
														{formatBytes(art.size_bytes)}
													</span>
													<button
														type="button"
														title="Delete only {art.rel_path}"
														onclick={() => hibernateSingle(project.path, art.rel_path)}
														class="rounded p-1 transition hover:opacity-75"
														style="color: var(--danger);"
													>
														<Trash2 class="h-3.5 w-3.5" />
													</button>
												</div>
											</div>
										{/each}
									</div>
								{/if}
							</div>
						{/if}
					</div>
				{/each}
			</div>
		{/if}
	</div>
	{:else}
		<div
			class="divide-y overflow-hidden rounded-xl border"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			{#each wb.caches as cache (cache.id)}
				<div class="flex items-center justify-between gap-4 px-4 py-3.5">
					<div class="min-w-0 flex-1">
						<div class="flex items-center gap-2">
							<span class="text-sm font-semibold" style="color: var(--text-primary);">
								{cache.name}
							</span>
							<span
								class="rounded px-1.5 py-0.5 text-[10px] font-medium"
								style="background-color: var(--bg-app); color: var(--text-secondary);"
							>
								{cache.ecosystem}
							</span>
						</div>
						<div class="mt-0.5 text-xs" style="color: var(--text-secondary);">
							{cache.description}
						</div>
						<div class="selectable mt-0.5 truncate font-mono text-[11px]" style="color: var(--text-muted);">
							{cache.path}
						</div>
					</div>

					<div class="flex items-center gap-3">
						<span
							class="min-w-20 text-right text-sm font-semibold tabular-nums"
							style="color: {cache.size_bytes > 0 ? 'var(--text-primary)' : 'var(--text-muted)'};"
						>
							{cache.exists ? formatBytes(cache.size_bytes) : 'Not found'}
						</span>

						{#if cache.exists}
							<button
								type="button"
								title="Reveal folder"
								onclick={() => api.revealInFileManager(cache.path)}
								class="rounded-md p-1.5 transition hover:opacity-75"
								style="color: var(--text-secondary);"
							>
								<FolderOpen class="h-3.5 w-3.5" />
							</button>
						{/if}

						<button
							type="button"
							disabled={!cache.exists || cache.size_bytes === 0 || cleaningCacheId === cache.id}
							onclick={() => askCleanCache(cache.id, cache.name, cache.size_bytes)}
							class="inline-flex items-center gap-1 rounded-md border px-2.5 py-1.5 text-xs font-medium transition hover:opacity-85 disabled:opacity-35"
							style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
						>
							<Trash2 class="h-3.5 w-3.5" />
							{cleaningCacheId === cache.id ? 'Cleaning...' : 'Clean'}
						</button>
					</div>
				</div>
			{/each}
		</div>
	{/if}
</div>

<ConfirmDialog
	bind:open={bulkConfirmOpen}
	title="Hibernate {selectedPaths.size} selected projects?"
	confirmLabel="Hibernate & Free {formatBytes(selectedReclaimable)}"
	destructive={wb.config.delete_mode === 'permanent'}
	onConfirm={runBulkHibernate}
	onCancel={() => {}}
>
	<p>
		Regenerable dependency and build folders will be removed from
		<strong>{selectedPaths.size}</strong> projects using
		<strong>{wb.config.delete_mode === 'permanent' ? 'Permanent Delete' : 'OS Trash'}</strong>.
	</p>
	<p class="mt-2 text-xs" style="color: var(--text-muted);">
		Your source files, git commits, and environment configs remain untouched.
	</p>
</ConfirmDialog>

<ConfirmDialog
	bind:open={cacheConfirmOpen}
	title="Clean {pendingCacheName}?"
	confirmLabel="Clean & Free {formatBytes(pendingCacheSize)}"
	destructive={wb.config.delete_mode === 'permanent'}
	onConfirm={doCleanCache}
	onCancel={() => {}}
>
	<p>
		This will clear cached archives in <strong>{pendingCacheName}</strong>. Packages will be
		re-downloaded automatically the next time you install dependencies.
	</p>
</ConfirmDialog>
