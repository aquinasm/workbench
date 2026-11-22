<script lang="ts">
	import { goto } from '$app/navigation';
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { formatBytes, formatIdleDays, kindLabel } from '$lib/format';
	import TechIcon from '$lib/components/TechIcon.svelte';
	import type { ProjectInfo, ProjectKind } from '$lib/types';
	import {
		Search,
		FolderPlus,
		Terminal,
		ArrowUpRight,
		X,
		Folder,
		FolderOpen,
		Globe
	} from 'lucide-svelte';

	let query = $state('');
	let kindFilter = $state<'all' | ProjectKind>('all');
	let workspaceFilter = $state<string>('all');

	const kindOrder: ProjectKind[] = ['laravel', 'sveltekit', 'flutter', 'rust', 'node', 'php'];

	function folderLabel(path: string): string {
		const parts = path.replace(/[/\\]+$/, '').split(/[/\\]/);
		return parts[parts.length - 1] || path;
	}

	function isInsideRoot(projectPath: string, root: string): boolean {
		const normRoot = root.replace(/[/\\]+$/, '');
		return (
			projectPath === normRoot ||
			projectPath.startsWith(`${normRoot}/`) ||
			projectPath.startsWith(`${normRoot}\\`)
		);
	}

	const sitesByPath = $derived(new Map((wb.config.sites ?? []).map((s) => [s.project_path, s])));

	const countsByRoot = $derived.by(() => {
		const map = new Map<string, number>();
		for (const root of wb.config.scan_roots) {
			let c = 0;
			for (const p of wb.projects) {
				if (isInsideRoot(p.path, root)) c++;
			}
			map.set(root, c);
		}
		return map;
	});

	const filtered = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return wb.projects
			.filter((p) => {
				if (workspaceFilter !== 'all' && !isInsideRoot(p.path, workspaceFilter)) {
					return false;
				}
				if (q && !p.name.toLowerCase().includes(q) && !p.path.toLowerCase().includes(q)) {
					return false;
				}
				if (
					kindFilter !== 'all' &&
					p.kind !== kindFilter &&
					!p.secondary_kinds.includes(kindFilter)
				) {
					return false;
				}
				return true;
			})
			.sort((a, b) => a.name.localeCompare(b.name));
	});

	const grouped = $derived.by(() => {
		const map = new Map<ProjectKind, ProjectInfo[]>();
		for (const p of filtered) {
			const list = map.get(p.kind) ?? [];
			list.push(p);
			map.set(p.kind, list);
		}
		return kindOrder
			.filter((k) => map.has(k))
			.map((k) => ({ kind: k, projects: map.get(k)! }));
	});

	function openProjectDetail(path: string) {
		goto(`/projects/detail?path=${encodeURIComponent(path)}`);
	}
</script>

<div class="mx-auto max-w-6xl space-y-6">
	<header
		class="flex flex-wrap items-end justify-between gap-3 border-b pb-4"
		style="border-color: var(--border-subtle);"
	>
		<div>
			<h1 class="text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
				Projects
			</h1>
		</div>
		<button type="button" onclick={() => wb.addWorkspaceFolder()} class="wb-btn-primary">
			<FolderPlus class="h-3.5 w-3.5" />
			Add Workspace Folder
		</button>
	</header>

	<!-- Multiple Workspace Folders Switcher -->
	{#if wb.config.scan_roots.length > 0}
		<div class="flex flex-wrap items-center gap-2">
			<button
				type="button"
				onclick={() => (workspaceFilter = 'all')}
				class="inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition hover:bg-[var(--bg-surface-hover)]"
				style="
					background-color: {workspaceFilter === 'all' ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
					color: {workspaceFilter === 'all' ? 'var(--accent)' : 'var(--text-secondary)'};
				"
			>
				<span>All Workspaces</span>
				<span class="font-mono text-[11px] opacity-75">
					{wb.projects.length}
				</span>
			</button>

			{#each wb.config.scan_roots as root (root)}
				{@const active = workspaceFilter === root}
				{@const count = countsByRoot.get(root) ?? 0}
				<div
					class="inline-flex items-center rounded-full transition"
					style="
						background-color: {active ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
					"
				>
					<button
						type="button"
						title={root}
						onclick={() => (workspaceFilter = root)}
						class="inline-flex items-center gap-1.5 rounded-l-full py-1.5 pl-3.5 pr-2 text-xs font-medium"
						style="color: {active ? 'var(--accent)' : 'var(--text-secondary)'};"
					>
						<Folder class="h-3.5 w-3.5" style="color: {active ? 'var(--accent)' : 'var(--text-muted)'};" />
						<span>{folderLabel(root)}</span>
						<span class="font-mono text-[11px] opacity-75">
							{count}
						</span>
					</button>
					<button
						type="button"
						title="Remove workspace folder"
						onclick={() => {
							if (workspaceFilter === root) workspaceFilter = 'all';
							wb.removeWorkspaceFolder(root);
						}}
						class="mr-1 rounded-full p-1 transition hover:bg-[var(--bg-surface-hover)] hover:text-[var(--danger)]"
						style="color: var(--text-muted);"
					>
						<X class="h-3 w-3" />
					</button>
				</div>
			{/each}
		</div>
	{/if}

	{#if wb.config.scan_roots.length === 0}
		<div
			class="border p-12 text-center space-y-3"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			<div class="text-sm font-semibold" style="color: var(--text-primary);">
				No workspace folders added yet
			</div>
			<p class="mx-auto max-w-md text-xs" style="color: var(--text-secondary);">
				Add one or more workspace folders (e.g. <code class="font-mono">~/Projects/Work</code>,
				<code class="font-mono">~/Projects/Personal</code>) to group and configure your projects.
			</p>
			<button type="button" onclick={() => wb.addWorkspaceFolder()} class="wb-btn-primary">
				<FolderPlus class="h-3.5 w-3.5" />
				Add Workspace Folder
			</button>
		</div>
	{:else if wb.scanningProjects && wb.projects.length === 0}
		<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
			{#each Array(6) as _}
				<div
					class="h-28 animate-pulse border p-4"
					style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
				></div>
			{/each}
		</div>
	{:else}
		<!-- Search & Stack Filter Bar -->
		<div class="flex flex-wrap items-center gap-2.5">
			<div class="relative min-w-56 flex-1">
				<Search
					class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
					style="color: var(--text-muted);"
				/>
				<input
					type="search"
					bind:value={query}
					placeholder="Search {filtered.length} projects by name or path..."
					class="w-full py-2 pl-9 pr-3 text-xs outline-none"
					style="color: var(--text-primary);"
				/>
			</div>
			<select
				bind:value={kindFilter}
				aria-label="Filter by app type"
				class="px-3 py-2 text-xs"
				style="color: var(--text-primary);"
			>
				<option value="all">All App Types</option>
				<option value="laravel">Laravel</option>
				<option value="sveltekit">SvelteKit</option>
				<option value="flutter">Flutter</option>
				<option value="rust">Rust</option>
				<option value="node">Node</option>
				<option value="php">PHP</option>
			</select>
		</div>

		<!-- Grouped Project Grids -->
		{#if grouped.length === 0}
			<div
				class="border p-10 text-center font-mono text-xs"
				style="background-color: var(--bg-surface); border-color: var(--border-subtle); color: var(--text-muted);"
			>
				NO PROJECTS MATCH THE CURRENT FILTER
			</div>
		{:else}
			<div class="space-y-7">
				{#each grouped as group (group.kind)}
					<section class="space-y-3">
						<div class="flex items-center justify-between border-b pb-2" style="border-color: var(--border-subtle);">
							<div class="flex items-center gap-2">
								<TechIcon
									name={group.kind}
									class="h-4 w-4"
								/>
								<h2 class="font-mono text-xs font-semibold uppercase tracking-[0.1em]" style="color: var(--text-primary);">
									{kindLabel(group.kind)}
								</h2>
								<span class="font-mono text-[11px]" style="color: var(--text-muted);">
									({group.projects.length})
								</span>
							</div>
						</div>

						<div class="wb-grid-cards">
							{#each group.projects as project (project.path)}
								{@const site = sitesByPath.get(project.path)}
								{@const pinEntries = Object.entries(project.version_hints)}
								{@const isRecent = !project.hibernated && project.idle_days <= 3}
								<div
									role="button"
									tabindex="0"
									onclick={() => openProjectDetail(project.path)}
									onkeydown={(e) => {
										if (e.key === 'Enter' || e.key === ' ') {
											e.preventDefault();
											openProjectDetail(project.path);
										}
									}}
									class="group flex flex-col justify-between rounded border p-4 text-left transition hover:border-[var(--border-strong)] hover:bg-[var(--bg-surface-hover)] cursor-pointer"
									style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
								>
									<div class="space-y-3">
										<div class="flex items-start gap-3">
											<div
												class="flex h-9 w-9 shrink-0 items-center justify-center rounded border transition group-hover:border-[var(--border-strong)]"
												style="background-color: var(--bg-elevated); border-color: var(--border-subtle);"
											>
												<TechIcon name={project.kind} class="h-4.5 w-4.5" />
											</div>

											<div class="min-w-0 flex-1">
												<div class="flex items-center justify-between gap-2">
													<span
														class="truncate text-[14px] font-semibold leading-snug"
														style="color: var(--text-primary);"
													>
														{project.name}
													</span>
													<ArrowUpRight
														class="h-3.5 w-3.5 shrink-0 opacity-35 transition-transform duration-150 group-hover:translate-x-0.5 group-hover:-translate-y-0.5 group-hover:opacity-100"
														style="color: var(--accent);"
													/>
												</div>

												<div class="mt-0.5 flex items-center gap-1.5 truncate font-mono text-[11.5px]" style="color: var(--text-muted);">
													{#if site}
														<Globe class="h-3 w-3 shrink-0" style="color: var(--accent);" />
														<span class="truncate" style="color: var(--accent);">{site.domain}</span>
													{:else}
														<Folder class="h-3 w-3 shrink-0 opacity-60" />
														<span class="truncate">
															{folderLabel(project.path.slice(0, project.path.lastIndexOf('/')) || project.path)}
														</span>
													{/if}
												</div>
											</div>
										</div>

										{#if pinEntries.length > 0 || project.secondary_kinds.length > 0}
											<div class="flex flex-wrap items-center gap-1.5">
												{#each project.secondary_kinds.slice(0, 2) as sec}
													<span
														class="inline-flex items-center gap-1 rounded border px-1.5 py-0.5 font-mono text-[10.5px]"
														style="border-color: var(--border-subtle); background-color: var(--bg-elevated); color: var(--text-secondary);"
													>
														<TechIcon name={sec} class="h-3 w-3" />
														{kindLabel(sec)}
													</span>
												{/each}
												{#each pinEntries.slice(0, 3) as [tool, ver]}
													<span
														class="rounded border px-1.5 py-0.5 font-mono text-[10.5px]"
														style="border-color: var(--border-subtle); background-color: var(--bg-elevated); color: var(--text-secondary);"
													>
														{tool} {ver.replace(/^ext-/, '')}
													</span>
												{/each}
											</div>
										{/if}
									</div>

									<div
										class="mt-3.5 flex items-center justify-between border-t pt-2.5 font-mono text-[11.5px]"
										style="border-color: var(--border-subtle); color: var(--text-muted);"
									>
										<div class="flex items-center gap-1.5 min-w-0">
											<span
												class="h-1.5 w-1.5 shrink-0 rounded-full"
												style="background-color: {project.hibernated ? 'var(--text-muted)' : isRecent ? 'var(--success)' : 'var(--border-strong)'};"
												title={project.hibernated ? 'Hibernated' : isRecent ? 'Recently active' : 'Idle'}
											></span>
											<span
												class="truncate"
												style="color: {project.hibernated ? 'var(--text-muted)' : 'var(--text-secondary)'};"
											>
												{#if project.hibernated}
													Hibernated
												{:else if project.total_reclaimable_bytes > 0}
													{formatBytes(project.total_reclaimable_bytes)}
												{:else if wb.scanningProjects}
													Sizing...
												{:else}
													Active
												{/if}
											</span>
											<span class="opacity-50">·</span>
											<span class="shrink-0">{formatIdleDays(project.idle_days)}</span>
										</div>

										<div class="flex items-center gap-1">
											<button
												type="button"
												title="Reveal in Folder"
												onclick={(e) => {
													e.stopPropagation();
													api.revealInFileManager(project.path);
												}}
												class="flex h-6 w-6 items-center justify-center rounded border border-transparent transition hover:border-[var(--border-subtle)] hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)]"
												style="color: var(--text-muted);"
											>
												<FolderOpen class="h-3.5 w-3.5" />
											</button>
											<button
												type="button"
												title="Open in Terminal"
												onclick={(e) => {
													e.stopPropagation();
													api.openInTerminal(project.path);
												}}
												class="flex h-6 w-6 items-center justify-center rounded border border-transparent transition hover:border-[var(--border-subtle)] hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)]"
												style="color: var(--text-muted);"
											>
												<Terminal class="h-3.5 w-3.5" />
											</button>
										</div>
									</div>
								</div>
							{/each}
						</div>
					</section>
				{/each}
			</div>
		{/if}
	{/if}
</div>
