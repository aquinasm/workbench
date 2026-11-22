<script lang="ts">
	import { goto } from '$app/navigation';
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { buildSiteUrl, formatBytes } from '$lib/format';
	import { Search, ArrowRight } from 'lucide-svelte';

	interface Props {
		open: boolean;
	}

	let { open = $bindable(false) }: Props = $props();

	let dialogEl: HTMLDialogElement | undefined = $state();
	let inputEl: HTMLInputElement | undefined = $state();
	let query = $state('');
	let activeIdx = $state(0);

	interface PaletteItem {
		id: string;
		group: string;
		label: string;
		hint?: string;
		action: () => unknown;
	}

	const allItems = $derived.by<PaletteItem[]>(() => {
		if (!open) return [];
		const sitesByPath = new Map((wb.config.sites ?? []).map((s) => [s.project_path, s]));
		const base: PaletteItem[] = [
			{ id: 'nav-dash', group: 'Navigate', label: 'Dashboard', hint: '⌘1', action: () => goto('/') },
			{ id: 'nav-proj', group: 'Navigate', label: 'Projects', hint: '⌘2', action: () => goto('/projects') },
			{ id: 'nav-db', group: 'Navigate', label: 'Databases', hint: '⌘3', action: () => goto('/databases') },
			{ id: 'nav-svc', group: 'Navigate', label: 'Local Services', hint: '⌘4', action: () => goto('/services') },
			{ id: 'nav-rt', group: 'Navigate', label: 'Runtimes (PHP, Node, Flutter, Rust)', hint: '⌘5', action: () => goto('/runtimes') },
			{ id: 'nav-stor', group: 'Navigate', label: 'Storage & Caches', hint: '⌘6', action: () => goto('/storage') },
			{ id: 'nav-set', group: 'Navigate', label: 'Settings', hint: '⌘7', action: () => goto('/settings') },
			{ id: 'nav-about', group: 'Navigate', label: 'About Workbench', hint: '⌘8', action: () => goto('/about') },
			{
				id: 'act-add-ws',
				group: 'Actions',
				label: 'Add Workspace Folder',
				hint: 'Scan new directory',
				action: () => wb.addWorkspaceFolder()
			},
			{
				id: 'act-rescan',
				group: 'Actions',
				label: 'Rescan Workspaces & Caches',
				hint: '⌘R',
				action: () => {
					wb.scanProjects();
					wb.refreshCaches();
				}
			},
			{
				id: 'act-start-all',
				group: 'Services',
				label: 'Start All Available Services',
				action: () => wb.startAllServices()
			},
			{
				id: 'act-stop-all',
				group: 'Services',
				label: 'Stop All Running Services (Graceful)',
				action: () => wb.stopAllServices()
			}
		];

		for (const p of wb.projects) {
			const sizeHint = p.hibernated ? 'hibernated' : formatBytes(p.total_reclaimable_bytes);
			base.push({
				id: `proj-open-${p.path}`,
				group: 'Workbench',
				label: `Open ${p.name}`,
				hint: `${p.kind} · ${sizeHint}`,
				action: () => goto(`/projects/detail?path=${encodeURIComponent(p.path)}`)
			});
			base.push({
				id: `proj-finder-${p.path}`,
				group: 'Files',
				label: `Reveal ${p.name} in Finder / Explorer`,
				hint: p.path,
				action: () => api.revealInFileManager(p.path)
			});
			base.push({
				id: `proj-term-${p.path}`,
				group: 'Terminal',
				label: `Open ${p.name} in Terminal`,
				hint: p.kind,
				action: () => api.openInTerminal(p.path)
			});

			const site = sitesByPath.get(p.path);
			if (site) {
				const url = buildSiteUrl(site, wb.networkStatus);
				base.push({
					id: `proj-site-${p.path}`,
					group: 'Browser',
					label: `Open ${p.name} (${site.domain})`,
					hint: url,
					action: () => api.openExternalUrl(url)
				});
			}

			if (p.hibernated) {
				base.push({
					id: `proj-wake-${p.path}`,
					group: 'Storage',
					label: `Wake ${p.name}`,
					hint: 'Restore dependencies',
					action: () => wb.wakeOne(p.path)
				});
			} else if (p.total_reclaimable_bytes > 0) {
				base.push({
					id: `proj-hib-${p.path}`,
					group: 'Storage',
					label: `Hibernate ${p.name}`,
					hint: `Free ${formatBytes(p.total_reclaimable_bytes)}`,
					action: () => wb.hibernateOne(p.path)
				});
			}
		}

		return base;
	});

	const items = $derived.by<PaletteItem[]>(() => {
		if (!open) return [];
		const q = query.trim().toLowerCase();
		if (!q) return allItems.slice(0, 14);
		return allItems
			.filter(
				(i) =>
					i.label.toLowerCase().includes(q) ||
					i.group.toLowerCase().includes(q) ||
					(i.hint?.toLowerCase().includes(q) ?? false)
			)
			.slice(0, 16);
	});

	$effect(() => {
		if (!dialogEl) return;
		if (open && !dialogEl.open) {
			query = '';
			activeIdx = 0;
			dialogEl.showModal();
			queueMicrotask(() => inputEl?.focus());
		} else if (!open && dialogEl.open) {
			dialogEl.close();
		}
	});

	$effect(() => {
		if (activeIdx >= items.length) {
			activeIdx = Math.max(0, items.length - 1);
		}
	});

	function handleClose() {
		if (dialogEl) dialogEl.returnValue = '';
		open = false;
	}

	async function runItem(item: PaletteItem) {
		open = false;
		try {
			await item.action();
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		}
	}

	function onKeyDown(e: KeyboardEvent) {
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			activeIdx = items.length ? (activeIdx + 1) % items.length : 0;
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			activeIdx = items.length ? (activeIdx - 1 + items.length) % items.length : 0;
		} else if (e.key === 'Enter' && items[activeIdx]) {
			e.preventDefault();
			runItem(items[activeIdx]);
		}
	}
</script>

<dialog
	bind:this={dialogEl}
	aria-label="Command palette"
	onclose={handleClose}
	onclick={(e) => {
		if (e.target === dialogEl) open = false;
	}}
	style="max-width: 580px;"
>
	<div
		class="flex items-center gap-3 border-b px-4 py-3.5"
		style="border-color: var(--border-subtle);"
	>
		<Search class="h-4 w-4 shrink-0" style="color: var(--accent);" />
		<input
			bind:this={inputEl}
			type="text"
			bind:value={query}
			onkeydown={onKeyDown}
			placeholder="Type a command, page, or project name..."
			class="wb-bare-input w-full text-[14px]"
			style="color: var(--text-primary);"
		/>
		<kbd
			class="rounded-[6px] border px-1.5 py-0.5 font-mono text-[11px]"
			style="border-color: var(--border-subtle); background-color: var(--bg-elevated); color: var(--text-secondary);"
		>
			ESC
		</kbd>
	</div>

	<div class="max-h-96 overflow-y-auto p-1.5 space-y-0.5">
		{#if items.length === 0}
			<div class="p-8 text-center font-mono text-xs" style="color: var(--text-muted);">
				NO MATCHING COMMANDS
			</div>
		{:else}
			{#each items as item, idx (item.id)}
				{@const selected = idx === activeIdx}
				<button
					type="button"
					onmouseenter={() => (activeIdx = idx)}
					onclick={() => runItem(item)}
					class="flex w-full items-center justify-between gap-3 rounded-[10px] px-3 py-2.5 text-left text-[13.5px]"
					style="
						background-color: {selected ? 'var(--accent-soft)' : 'transparent'};
					"
				>
					<span class="flex items-center gap-3 min-w-0">
						<span class="wb-kicker w-20 shrink-0">{item.group}</span>
						<span class="truncate font-medium" style="color: var(--text-primary);">
							{item.label}
						</span>
					</span>
					<span class="flex shrink-0 items-center gap-1.5 font-mono text-xs" style="color: var(--text-muted);">
						{#if item.hint}
							<span class="max-w-44 truncate">{item.hint}</span>
						{/if}
						{#if selected}
							<ArrowRight class="h-3.5 w-3.5 shrink-0" style="color: var(--accent);" />
						{/if}
					</span>
				</button>
			{/each}
		{/if}
	</div>

	<div
		class="flex items-center justify-between border-t px-4 py-2.5 font-mono text-[11px]"
		style="border-color: var(--border-subtle); background-color: var(--bg-sidebar); color: var(--text-muted);"
	>
		<span>↑↓ NAVIGATE · ↵ RUN · ESC CLOSE</span>
		<span>WORKBENCH · ⌘K</span>
	</div>
</dialog>
