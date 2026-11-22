<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import {
		LayoutDashboard,
		FolderKanban,
		HardDrive,
		Cpu,
		Server,
		Database,
		Settings,
		Info,
		RefreshCw,
		Terminal,
		Search,
		Sun,
		Moon,
		ArrowUpRight
	} from 'lucide-svelte';

	interface Props {
		onOpenPalette: () => void;
	}

	let { onOpenPalette }: Props = $props();

	const navItems = [
		{ href: '/', label: 'Dashboard', shortcut: '⌘1', icon: LayoutDashboard },
		{ href: '/projects', label: 'Projects', shortcut: '⌘2', icon: FolderKanban },
		{ href: '/databases', label: 'Databases', shortcut: '⌘3', icon: Database },
		{ href: '/services', label: 'Services', shortcut: '⌘4', icon: Server },
		{ href: '/runtimes', label: 'Runtimes', shortcut: '⌘5', icon: Cpu },
		{ href: '/storage', label: 'Storage', shortcut: '⌘6', icon: HardDrive },
		{ href: '/settings', label: 'Settings', shortcut: '⌘7', icon: Settings },
		{ href: '/about', label: 'About', shortcut: '⌘8', icon: Info }
	];

	const runningServicesCount = $derived(
		wb.services.filter((s) => s.status === 'running' || s.status === 'external_running').length
	);

	let osPrefersLight = $state(false);

	onMount(() => {
		const mql = window.matchMedia('(prefers-color-scheme: light)');
		osPrefersLight = mql.matches;
		const onChange = (e: MediaQueryListEvent) => {
			osPrefersLight = e.matches;
		};
		mql.addEventListener('change', onChange);
		return () => mql.removeEventListener('change', onChange);
	});

	const isLight = $derived.by(() => {
		if (wb.config.theme === 'light') return true;
		if (wb.config.theme === 'dark') return false;
		return osPrefersLight;
	});

	function toggleTheme() {
		const next = isLight ? 'dark' : 'light';
		wb.saveConfig({ theme: next });
	}
</script>

<aside
	class="flex h-full w-60 shrink-0 flex-col border-r"
	style="background-color: var(--bg-sidebar); border-color: var(--border-subtle);"
>
	<!-- Dedicated macOS traffic-light drag strip -->
	<div data-tauri-drag-region class="h-9 w-full shrink-0"></div>

	<!-- Brand Header -->
	<div class="flex items-center justify-between px-4 pb-3">
		<div class="flex items-center gap-2.5">
			<svg
				xmlns="http://www.w3.org/2000/svg"
				viewBox="0 0 1024 1024"
				fill="none"
				class="h-7 w-7 shrink-0"
				aria-hidden="true"
			>
				<rect x="48" y="48" width="928" height="928" rx="224" fill="var(--bg-app)" />
				<rect x="58" y="58" width="908" height="908" rx="214" stroke="var(--border-subtle)" stroke-width="24" />
				<path
					d="M 211 364 L 373 452 L 211 540"
					stroke="var(--text-primary)"
					stroke-width="64"
					stroke-linecap="round"
					stroke-linejoin="round"
				/>
				<rect x="410" y="634" width="174" height="58" rx="29" fill="var(--accent)" />
				<path
					d="M 621 364 L 669 540 L 717 404 L 765 540 L 813 364"
					stroke="var(--text-primary)"
					stroke-width="62"
					stroke-linecap="round"
					stroke-linejoin="round"
				/>
			</svg>
			<span
				class="text-sm font-semibold tracking-[-0.02em]"
				style="color: var(--text-primary);"
			>
				Workbench
			</span>
		</div>
		<div class="flex items-center gap-1">
			<button
				type="button"
				title={isLight ? 'Switch to Dark theme' : 'Switch to Light theme'}
				onclick={toggleTheme}
				class="rounded-full p-1.5 transition hover:bg-[var(--bg-surface-hover)]"
				style="color: var(--text-muted);"
			>
				{#if isLight}
					<Moon class="h-4 w-4" />
				{:else}
					<Sun class="h-4 w-4" />
				{/if}
			</button>
			<button
				type="button"
				title="Refresh environment (⌘R)"
				onclick={() => {
					wb.refreshServices();
					wb.refreshRuntimes();
					wb.scanProjects();
				}}
				disabled={wb.scanningProjects || wb.loadingServices}
				class="rounded-full p-1.5 transition hover:bg-[var(--bg-surface-hover)] disabled:opacity-40"
				style="color: var(--text-muted);"
			>
				<RefreshCw class="h-4 w-4 {wb.scanningProjects || wb.loadingServices ? 'animate-spin' : ''}" />
			</button>
		</div>
	</div>

	<!-- Command Palette Trigger -->
	<div class="px-3 pb-3">
		<button
			type="button"
			onclick={onOpenPalette}
			class="flex w-full items-center justify-between rounded-[10px] px-3 py-2 text-[13px] transition hover:bg-[var(--bg-surface-hover)]"
			style="background-color: var(--bg-elevated); color: var(--text-muted);"
		>
			<span class="flex items-center gap-2">
				<Search class="h-3.5 w-3.5" />
				<span>Command...</span>
			</span>
			<kbd
				class="rounded-[6px] px-1.5 py-0.5 font-mono text-[11px]"
				style="background-color: var(--bg-elevated); color: var(--text-secondary);"
			>
				⌘K
			</kbd>
		</button>
	</div>

	<!-- Navigation -->
	<nav class="flex-1 space-y-1 overflow-y-auto px-3 py-1">
		{#each navItems as item}
			{@const active =
				item.href === '/'
					? $page.url.pathname === '/'
					: $page.url.pathname === item.href || $page.url.pathname.startsWith(`${item.href}/`)}
			{@const Icon = item.icon}
			<a
				href={item.href}
				title="{item.label} ({item.shortcut})"
				class="group relative flex items-center justify-between rounded-[10px] px-3 py-2 text-sm font-medium transition hover:bg-[var(--bg-surface-hover)]"
				style="
					background-color: {active ? 'var(--accent-soft)' : ''};
					color: {active ? 'var(--text-primary)' : 'var(--text-secondary)'};
				"
			>
				<span class="flex items-center gap-3">
					<Icon
						class="h-4 w-4 shrink-0"
						style="color: {active ? 'var(--accent)' : 'var(--text-muted)'};"
					/>
					<span>{item.label}</span>
				</span>

				{#if item.href === '/services' && runningServicesCount > 0}
					<span
						class="inline-flex items-center gap-1.5 font-mono text-xs font-semibold"
						style="color: var(--success);"
					>
						<span class="h-2 w-2 rounded-full bg-emerald-400"></span>
						{runningServicesCount}
					</span>
				{:else if item.href === '/projects' && wb.projects.length > 0}
					<span class="font-mono text-xs tabular-nums" style="color: var(--text-muted);">
						{wb.projects.length}
					</span>
				{/if}
			</a>
		{/each}
	</nav>

	<!-- Version & Update Status Footer -->
	<div class="border-t p-3 space-y-2.5" style="border-color: var(--border-subtle);">
		{#if wb.terminalLogs.length > 0 || wb.terminalRunning}
			<button
				type="button"
				onclick={() => (wb.terminalOpen = !wb.terminalOpen)}
				class="flex w-full items-center justify-between rounded-[10px] px-3 py-1.5 font-mono text-xs font-medium transition hover:bg-[var(--bg-surface-hover)]"
				style="background-color: var(--accent-soft); color: var(--accent);"
			>
				<span class="flex items-center gap-1.5">
					<Terminal class="h-3.5 w-3.5" />
					{wb.terminalRunning ? 'Waking...' : 'Wake Output'}
				</span>
				{#if wb.terminalRunning}
					<span class="h-2 w-2 rounded-full animate-pulse" style="background-color: var(--accent);"></span>
				{/if}
			</button>
		{/if}

		<div class="px-1 space-y-1.5">
			<a
				href="/about"
				class="flex items-center justify-between rounded-lg px-1 py-0.5 transition hover:bg-[var(--bg-surface-hover)]"
				title="About Workbench (⌘8)"
			>
				<span class="wb-kicker">Version</span>
				<span class="font-mono text-xs font-semibold" style="color: var(--text-primary);">
					v{wb.updateInfo.current_version}
				</span>
			</a>
			<div class="flex items-center justify-between pt-0.5 text-xs">
				{#if wb.checkingUpdates}
					<span class="inline-flex items-center gap-1.5 font-mono text-[11px]" style="color: var(--text-muted);">
						<RefreshCw class="h-3 w-3 animate-spin" />
						Checking updates...
					</span>
				{:else if wb.updateInfo.update_available && wb.updateInfo.latest_version}
					<button
						type="button"
						onclick={() => wb.updateInfo.release_url && api.openExternalUrl(wb.updateInfo.release_url)}
						class="inline-flex w-full items-center justify-between rounded-full px-2.5 py-1 font-mono text-[11px] font-semibold transition hover:opacity-85"
						style="background-color: var(--accent-soft); color: var(--accent);"
					>
						<span>Update v{wb.updateInfo.latest_version}</span>
						<ArrowUpRight class="h-3 w-3" />
					</button>
				{:else}
					<span
						class="inline-flex items-center gap-1.5 font-mono text-[11px]"
						style="color: {wb.updateInfo.checked ? 'var(--success)' : 'var(--text-muted)'};"
					>
						<span
							class="h-1.5 w-1.5 rounded-full"
							style="background-color: {wb.updateInfo.checked ? 'var(--success)' : 'var(--text-muted)'};"
						></span>
						{wb.updateInfo.checked ? 'Up to date' : 'Check updates'}
					</span>
					<button
						type="button"
						title="Check GitHub Releases for updates"
						onclick={() => wb.checkForUpdates(true)}
						class="rounded-full p-1 transition hover:bg-[var(--bg-surface-hover)]"
						style="color: var(--text-muted);"
					>
						<RefreshCw class="h-3 w-3" />
					</button>
				{/if}
			</div>
		</div>
	</div>
</aside>
