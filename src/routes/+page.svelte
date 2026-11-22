<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { formatBytes, formatIdleDays, kindLabel } from '$lib/format';
	import type { SystemStats } from '$lib/types';
	import TechIcon from '$lib/components/TechIcon.svelte';
	import {
		FolderPlus,
		Play,
		Square,
		ArrowRight,
		Terminal,
		ExternalLink,
		Cpu,
		MemoryStick,
		Activity,
		ArrowDown,
		ArrowUp
	} from 'lucide-svelte';

	const MAX_SAMPLES = 40;
	const GRAPH_W = 280;
	const GRAPH_H = 74;

	let stats = $state<SystemStats | null>(null);
	let cpuHistory = $state<number[]>(Array(MAX_SAMPLES).fill(0));
	let memHistory = $state<number[]>(Array(MAX_SAMPLES).fill(0));
	let rxHistory = $state<number[]>(Array(MAX_SAMPLES).fill(0));
	let txHistory = $state<number[]>(Array(MAX_SAMPLES).fill(0));

	function pushSample(arr: number[], value: number): number[] {
		return [...arr.slice(1), value];
	}

	function buildGraphPaths(values: number[], maxVal: number): { line: string; area: string } {
		const n = values.length;
		if (n < 2) return { line: '', area: '' };
		const safeMax = maxVal > 0 ? maxVal : 1;
		const step = GRAPH_W / (n - 1);
		const padTop = 5;
		const usableH = GRAPH_H - padTop - 2;

		const pts = values.map((v, i) => {
			const clamped = Math.max(0, Math.min(safeMax, v));
			const x = +(i * step).toFixed(1);
			const y = +(padTop + usableH * (1 - clamped / safeMax)).toFixed(1);
			return `${x},${y}`;
		});

		const line = `M ${pts.join(' L ')}`;
		const area = `${line} L ${GRAPH_W},${GRAPH_H} L 0,${GRAPH_H} Z`;
		return { line, area };
	}

	function formatRate(bytesPerSec: number): string {
		if (bytesPerSec < 1024) return `${bytesPerSec} B/s`;
		if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
		if (bytesPerSec < 1024 * 1024 * 1024) return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`;
		return `${(bytesPerSec / (1024 * 1024 * 1024)).toFixed(2)} GB/s`;
	}

	function formatUptime(secs: number): string {
		const days = Math.floor(secs / 86400);
		const hours = Math.floor((secs % 86400) / 3600);
		const mins = Math.floor((secs % 3600) / 60);
		if (days > 0) return `${days}d ${hours}h`;
		if (hours > 0) return `${hours}h ${mins}m`;
		return `${mins}m`;
	}

	onMount(() => {
		let alive = true;
		let timer: ReturnType<typeof setTimeout> | null = null;

		async function pollStats() {
			if (!alive || document.hidden) return;
			try {
				const s = await api.getSystemStats();
				if (!alive) return;
				stats = s;
				cpuHistory = pushSample(cpuHistory, s.cpu_percent);
				memHistory = pushSample(memHistory, s.memory_percent);
				rxHistory = pushSample(rxHistory, s.net_rx_bytes_per_sec);
				txHistory = pushSample(txHistory, s.net_tx_bytes_per_sec);
			} catch {
				// Ignore transient telemetry errors
			} finally {
				if (alive && !document.hidden) {
					timer = setTimeout(pollStats, 1500);
				}
			}
		}

		function onVisibility() {
			if (timer) {
				clearTimeout(timer);
				timer = null;
			}
			if (!document.hidden && alive) {
				void pollStats();
			}
		}

		document.addEventListener('visibilitychange', onVisibility);
		void pollStats();
		return () => {
			alive = false;
			document.removeEventListener('visibilitychange', onVisibility);
			if (timer) clearTimeout(timer);
		};
	});

	const cpuPaths = $derived(buildGraphPaths(cpuHistory, 100));
	const memPaths = $derived(buildGraphPaths(memHistory, 100));
	const netPeak = $derived(Math.max(1024 * 16, ...rxHistory, ...txHistory));
	const rxPaths = $derived(buildGraphPaths(rxHistory, netPeak));
	const txPaths = $derived(buildGraphPaths(txHistory, netPeak));

	const runningServices = $derived(
		wb.services.filter((s) => s.status === 'running' || s.status === 'external_running')
	);
	const sortedServices = $derived(
		[...wb.services].sort((a, b) => {
			const aRun = a.status === 'running' || a.status === 'external_running' ? 0 : 1;
			const bRun = b.status === 'running' || b.status === 'external_running' ? 0 : 1;
			return aRun - bRun;
		})
	);
	const installedRuntimes = $derived(wb.runtimes.filter((r) => r.installed));
	const totalCacheBytes = $derived(wb.caches.reduce((sum, c) => sum + c.size_bytes, 0));

	const activeDefaults = $derived.by(() => {
		const tools = ['php', 'node', 'composer', 'flutter'];
		const list = tools.map((tool) => {
			const def =
				installedRuntimes.find((r) => r.tool === tool && r.is_default) ??
				installedRuntimes.find((r) => r.tool === tool);
			return {
				tool,
				version: def ? def.version.replace(/^ext-/, '') : null,
				isExternal: def?.is_external ?? false
			};
		});
		const rustDef = wb.rustup.toolchains.find((t) => t.is_default) ?? wb.rustup.toolchains[0];
		list.push({
			tool: 'rust',
			version: rustDef ? rustDef.name.split('-')[0] : null,
			isExternal: false
		});
		return list;
	});

	const recentProjects = $derived(
		[...wb.projects]
			.sort((a, b) => b.last_modified_epoch - a.last_modified_epoch)
			.slice(0, 6)
	);
</script>

<div class="mx-auto max-w-6xl space-y-6">
	<header
		class="flex flex-wrap items-end justify-between gap-4 border-b pb-4"
		style="border-color: var(--border-subtle);"
	>
		<div class="min-w-0">
			<h1 class="text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
				Dashboard
			</h1>
			{#if stats}
				<p class="mt-0.5 truncate font-mono text-[11px]" style="color: var(--text-muted);">
					{stats.os_name} · {stats.cpu_brand} · up {formatUptime(stats.uptime_secs)}
				</p>
			{/if}
		</div>
		<div class="flex flex-wrap items-center gap-2">
			<button type="button" onclick={() => wb.startAllServices()} class="wb-btn-primary">
				<Play class="h-3.5 w-3.5" />
				Start All Services
			</button>
			{#if runningServices.length > 0}
				<button type="button" onclick={() => wb.stopAllServices()} class="wb-btn-danger">
					<Square class="h-3.5 w-3.5" />
					Stop All ({runningServices.length})
				</button>
			{/if}
			<button type="button" onclick={() => wb.addWorkspaceFolder()} class="wb-btn">
				<FolderPlus class="h-3.5 w-3.5" />
				Add Workspace
			</button>
		</div>
	</header>

	<!-- Live System Telemetry Graphs: CPU, RAM, Network -->
	<section class="wb-telemetry-grid">
		<!-- CPU Usage -->
		<div class="rounded-2xl p-4 space-y-3" style="background-color: var(--bg-surface);">
			<div class="flex items-start justify-between gap-2">
				<div class="flex items-center gap-2">
					<div
						class="flex h-7 w-7 items-center justify-center rounded-lg"
						style="background-color: var(--accent-soft); color: var(--accent);"
					>
						<Cpu class="h-3.5 w-3.5" />
					</div>
					<div>
						<div class="wb-kicker">CPU</div>
						<div class="font-mono text-[11px]" style="color: var(--text-muted);">
							{stats ? `${stats.cpu_cores} cores · load ${stats.load_avg_one.toFixed(2)}` : 'Sampling...'}
						</div>
					</div>
				</div>
				<div class="text-right">
					<div
						class="font-mono text-2xl font-bold tracking-tight tabular-nums leading-none"
						style="color: var(--text-primary);"
					>
						{stats ? `${stats.cpu_percent.toFixed(1)}%` : '—'}
					</div>
				</div>
			</div>

			<div class="overflow-hidden rounded-xl" style="background-color: var(--bg-app);">
				<svg
					viewBox="0 0 {GRAPH_W} {GRAPH_H}"
					preserveAspectRatio="none"
					class="block h-[74px] w-full"
					aria-label="CPU usage graph"
				>
					<defs>
						<linearGradient id="cpuGrad" x1="0" y1="0" x2="0" y2="1">
							<stop offset="0%" stop-color="var(--accent)" stop-opacity="0.32" />
							<stop offset="100%" stop-color="var(--accent)" stop-opacity="0.02" />
						</linearGradient>
					</defs>
					<line
						x1="0"
						y1={GRAPH_H / 2}
						x2={GRAPH_W}
						y2={GRAPH_H / 2}
						stroke="var(--border-subtle)"
						stroke-dasharray="3 4"
						stroke-width="1"
					/>
					{#if cpuPaths.area}
						<path d={cpuPaths.area} fill="url(#cpuGrad)" />
						<path
							d={cpuPaths.line}
							fill="none"
							stroke="var(--accent)"
							stroke-width="1.75"
							stroke-linejoin="round"
							stroke-linecap="round"
						/>
					{/if}
				</svg>
			</div>
		</div>

		<!-- Memory (RAM) Usage -->
		<div class="rounded-2xl p-4 space-y-3" style="background-color: var(--bg-surface);">
			<div class="flex items-start justify-between gap-2">
				<div class="flex items-center gap-2">
					<div
						class="flex h-7 w-7 items-center justify-center rounded-lg"
						style="background-color: color-mix(in srgb, var(--success) 16%, transparent); color: var(--success);"
					>
						<MemoryStick class="h-3.5 w-3.5" />
					</div>
					<div>
						<div class="wb-kicker">Memory</div>
						<div class="font-mono text-[11px] tabular-nums" style="color: var(--text-muted);">
							{#if stats}
								{formatBytes(stats.memory_used_bytes)} / {formatBytes(stats.memory_total_bytes)}
							{:else}
								Sampling...
							{/if}
						</div>
					</div>
				</div>
				<div class="text-right">
					<div
						class="font-mono text-2xl font-bold tracking-tight tabular-nums leading-none"
						style="color: var(--text-primary);"
					>
						{stats ? `${stats.memory_percent.toFixed(1)}%` : '—'}
					</div>
				</div>
			</div>

			<div class="overflow-hidden rounded-xl" style="background-color: var(--bg-app);">
				<svg
					viewBox="0 0 {GRAPH_W} {GRAPH_H}"
					preserveAspectRatio="none"
					class="block h-[74px] w-full"
					aria-label="Memory usage graph"
				>
					<defs>
						<linearGradient id="memGrad" x1="0" y1="0" x2="0" y2="1">
							<stop offset="0%" stop-color="var(--success)" stop-opacity="0.30" />
							<stop offset="100%" stop-color="var(--success)" stop-opacity="0.02" />
						</linearGradient>
					</defs>
					<line
						x1="0"
						y1={GRAPH_H / 2}
						x2={GRAPH_W}
						y2={GRAPH_H / 2}
						stroke="var(--border-subtle)"
						stroke-dasharray="3 4"
						stroke-width="1"
					/>
					{#if memPaths.area}
						<path d={memPaths.area} fill="url(#memGrad)" />
						<path
							d={memPaths.line}
							fill="none"
							stroke="var(--success)"
							stroke-width="1.75"
							stroke-linejoin="round"
							stroke-linecap="round"
						/>
					{/if}
				</svg>
			</div>
		</div>

		<!-- Network Activity -->
		<div class="rounded-2xl p-4 space-y-3" style="background-color: var(--bg-surface);">
			<div class="flex items-start justify-between gap-2">
				<div class="flex items-center gap-2">
					<div
						class="flex h-7 w-7 items-center justify-center rounded-lg"
						style="background-color: var(--accent-soft); color: var(--accent);"
					>
						<Activity class="h-3.5 w-3.5" />
					</div>
					<div>
						<div class="wb-kicker">Network</div>
						<div class="font-mono text-[11px] tabular-nums" style="color: var(--text-muted);">
							Peak {formatRate(netPeak)}
						</div>
					</div>
				</div>
				<div class="flex flex-col items-end gap-0.5 font-mono text-xs tabular-nums">
					<span class="inline-flex items-center gap-1 font-semibold" style="color: var(--accent);">
						<ArrowDown class="h-3 w-3" />
						{stats ? formatRate(stats.net_rx_bytes_per_sec) : '—'}
					</span>
					<span class="inline-flex items-center gap-1" style="color: var(--success);">
						<ArrowUp class="h-3 w-3" />
						{stats ? formatRate(stats.net_tx_bytes_per_sec) : '—'}
					</span>
				</div>
			</div>

			<div class="overflow-hidden rounded-xl" style="background-color: var(--bg-app);">
				<svg
					viewBox="0 0 {GRAPH_W} {GRAPH_H}"
					preserveAspectRatio="none"
					class="block h-[74px] w-full"
					aria-label="Network activity graph"
				>
					<defs>
						<linearGradient id="rxGrad" x1="0" y1="0" x2="0" y2="1">
							<stop offset="0%" stop-color="var(--accent)" stop-opacity="0.28" />
							<stop offset="100%" stop-color="var(--accent)" stop-opacity="0.02" />
						</linearGradient>
						<linearGradient id="txGrad" x1="0" y1="0" x2="0" y2="1">
							<stop offset="0%" stop-color="var(--success)" stop-opacity="0.22" />
							<stop offset="100%" stop-color="var(--success)" stop-opacity="0.02" />
						</linearGradient>
					</defs>
					<line
						x1="0"
						y1={GRAPH_H / 2}
						x2={GRAPH_W}
						y2={GRAPH_H / 2}
						stroke="var(--border-subtle)"
						stroke-dasharray="3 4"
						stroke-width="1"
					/>
					{#if rxPaths.area}
						<path d={rxPaths.area} fill="url(#rxGrad)" />
						<path
							d={rxPaths.line}
							fill="none"
							stroke="var(--accent)"
							stroke-width="1.75"
							stroke-linejoin="round"
							stroke-linecap="round"
						/>
					{/if}
					{#if txPaths.area}
						<path d={txPaths.area} fill="url(#txGrad)" />
						<path
							d={txPaths.line}
							fill="none"
							stroke="var(--success)"
							stroke-width="1.5"
							stroke-linejoin="round"
							stroke-linecap="round"
						/>
					{/if}
				</svg>
			</div>
		</div>
	</section>

	<!-- Environment Overview Strip -->
	<section class="grid grid-cols-2 gap-3 lg:grid-cols-4">
		<a
			href="/services"
			class="rounded-2xl p-4 transition hover:bg-[var(--bg-surface-hover)]"
			style="background-color: var(--bg-surface);"
		>
			<div class="wb-kicker">Active Services</div>
			<div class="mt-1.5 font-mono text-2xl font-bold tracking-tight tabular-nums" style="color: var(--text-primary);">
				{runningServices.length}<span class="text-base font-normal" style="color: var(--text-muted);"> / {wb.services.length}</span>
			</div>
			<div class="mt-1 font-mono text-xs" style="color: {runningServices.length > 0 ? 'var(--success)' : 'var(--text-muted)'};">
				{runningServices.length > 0 ? 'Running daemons' : 'All stopped'}
			</div>
		</a>

		<a
			href="/runtimes"
			class="rounded-2xl p-4 transition hover:bg-[var(--bg-surface-hover)]"
			style="background-color: var(--bg-surface);"
		>
			<div class="wb-kicker">Runtimes</div>
			<div class="mt-1.5 font-mono text-2xl font-bold tracking-tight tabular-nums" style="color: var(--text-primary);">
				{installedRuntimes.length + wb.rustup.toolchains.length}
			</div>
			<div class="mt-1 font-mono text-xs" style="color: var(--text-muted);">
				PHP · Node · Flutter · Rust
			</div>
		</a>

		<a
			href="/projects"
			class="rounded-2xl p-4 transition hover:bg-[var(--bg-surface-hover)]"
			style="background-color: var(--bg-surface);"
		>
			<div class="wb-kicker">Projects</div>
			<div class="mt-1.5 font-mono text-2xl font-bold tracking-tight tabular-nums" style="color: var(--text-primary);">
				{wb.projects.length}
			</div>
			<div class="mt-1 font-mono text-xs" style="color: var(--text-muted);">
				{wb.config.scan_roots.length} {wb.config.scan_roots.length === 1 ? 'workspace' : 'workspaces'} · {(wb.config.sites ?? []).length} sites
			</div>
		</a>

		<a
			href="/storage"
			class="rounded-2xl p-4 transition hover:bg-[var(--bg-surface-hover)]"
			style="background-color: var(--bg-surface);"
		>
			<div class="wb-kicker">Reclaimable</div>
			<div class="mt-1.5 font-mono text-2xl font-bold tracking-tight tabular-nums" style="color: var(--text-primary);">
				{formatBytes(wb.totalReclaimable + totalCacheBytes)}
			</div>
			<div class="mt-1 font-mono text-xs" style="color: var(--text-muted);">
				Caches & build artifacts
			</div>
		</a>
	</section>

	<!-- Active Services -->
	<section class="space-y-3">
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-2">
				<span class="wb-kicker">Active Services</span>
				<span
					class="rounded-full px-2 py-0.5 font-mono text-[10.5px]"
					style="background-color: {runningServices.length > 0 ? 'color-mix(in srgb, var(--success) 16%, transparent)' : 'var(--bg-elevated)'}; color: {runningServices.length > 0 ? 'var(--success)' : 'var(--text-muted)'};"
				>
					{runningServices.length} running
				</span>
			</div>
			<a
				href="/services"
				class="inline-flex items-center gap-1 font-mono text-xs font-medium hover:underline"
				style="color: var(--accent);"
			>
				Manage Services
				<ArrowRight class="h-3.5 w-3.5" />
			</a>
		</div>

		<div class="wb-grid-cards">
			{#each sortedServices as svc (svc.id)}
				{@const isRunning = svc.status === 'running' || svc.status === 'external_running'}
				{@const canRun = svc.installed_managed || (svc.id !== 'phpmyadmin' && !!svc.host_binary)}
				<div
					class="flex items-center justify-between gap-3 rounded-2xl p-3.5 transition hover:bg-[var(--bg-surface-hover)]"
					style="background-color: var(--bg-surface);"
				>
					<div class="flex min-w-0 items-center gap-3">
						<div
							class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl"
							style="background-color: var(--bg-elevated);"
						>
							<TechIcon name={svc.id} class="h-5 w-5" />
						</div>
						<div class="min-w-0">
							<div class="truncate text-sm font-semibold" style="color: var(--text-primary);">
								{svc.name}
							</div>
							<div
								class="flex items-center gap-1.5 font-mono text-xs"
								style="color: {isRunning ? 'var(--success)' : 'var(--text-muted)'};"
							>
								<span
									class="h-2 w-2 shrink-0 rounded-full {isRunning ? 'bg-emerald-400' : 'bg-zinc-500'}"
								></span>
								<span class="truncate">v{svc.version} · {isRunning ? `:${svc.port}` : 'Stopped'}</span>
							</div>
						</div>
					</div>

					<div class="flex shrink-0 items-center gap-1.5">
						{#if isRunning && svc.url}
							<button
								type="button"
								title="Open {svc.url}"
								onclick={() => api.openExternalUrl(svc.url!)}
								class="wb-btn px-2"
							>
								<ExternalLink class="h-3.5 w-3.5" />
							</button>
						{/if}
						{#if isRunning}
							<button type="button" onclick={() => wb.stopService(svc.id)} class="wb-btn-danger">
								Stop
							</button>
						{:else}
							<button
								type="button"
								disabled={!canRun}
								onclick={() => wb.startService(svc.id)}
								class="wb-btn"
							>
								Start
							</button>
						{/if}
					</div>
				</div>
			{/each}
		</div>
	</section>

	<!-- Active Runtimes -->
	<section class="space-y-3">
		<div class="flex items-center justify-between">
			<span class="wb-kicker">Active Runtimes</span>
			<a
				href="/runtimes"
				class="inline-flex items-center gap-1 font-mono text-xs font-medium hover:underline"
				style="color: var(--accent);"
			>
				Manage Versions
				<ArrowRight class="h-3.5 w-3.5" />
			</a>
		</div>

		<div class="wb-grid-compact">
			{#each activeDefaults as rt (rt.tool)}
				<a
					href="/runtimes"
					class="flex items-center gap-3 rounded-2xl p-3.5 transition hover:bg-[var(--bg-surface-hover)]"
					style="background-color: var(--bg-surface);"
				>
					<div
						class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl"
						style="background-color: var(--bg-elevated); color: {rt.version ? 'var(--text-primary)' : 'var(--text-muted)'};"
					>
						<TechIcon name={rt.tool} class="h-4 w-4" />
					</div>
					<div class="min-w-0">
						<div class="font-mono text-[11px] uppercase" style="color: var(--text-muted);">
							{rt.tool}
						</div>
						<div class="truncate font-mono text-sm font-semibold" style="color: var(--text-primary);">
							{rt.version ?? '—'}
						</div>
					</div>
				</a>
			{/each}
		</div>
	</section>

	<!-- Recent Projects -->
	<section class="space-y-3">
		<div class="flex items-center justify-between">
			<span class="wb-kicker">Recent Projects</span>
			<a
				href="/projects"
				class="inline-flex items-center gap-1 font-mono text-xs font-medium hover:underline"
				style="color: var(--accent);"
			>
				All {wb.projects.length} Projects
				<ArrowRight class="h-3.5 w-3.5" />
			</a>
		</div>

		{#if recentProjects.length === 0}
			<div
				class="rounded-2xl p-8 text-center text-sm"
				style="background-color: var(--bg-surface); color: var(--text-secondary);"
			>
				No workspace folders added yet. Click <strong>Add Workspace</strong> above to get started.
			</div>
		{:else}
			<div class="wb-grid-cards">
				{#each recentProjects as project (project.path)}
					<div
						role="button"
						tabindex="0"
						onclick={() => goto(`/projects/detail?path=${encodeURIComponent(project.path)}`)}
						onkeydown={(e) => {
							if (e.key === 'Enter' || e.key === ' ') {
								e.preventDefault();
								goto(`/projects/detail?path=${encodeURIComponent(project.path)}`);
							}
						}}
						class="flex items-center justify-between gap-3 rounded-2xl p-3.5 text-left transition hover:bg-[var(--bg-surface-hover)] cursor-pointer"
						style="background-color: var(--bg-surface);"
					>
						<div class="flex min-w-0 items-center gap-2.5">
							<div
								class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl"
								style="background-color: var(--bg-elevated);"
							>
								<TechIcon name={project.kind} class="h-4 w-4" />
							</div>
							<div class="min-w-0">
								<div class="truncate text-sm font-semibold" style="color: var(--text-primary);">
									{project.name}
								</div>
								<div class="truncate font-mono text-xs" style="color: var(--text-muted);">
									{kindLabel(project.kind)} · {formatIdleDays(project.idle_days)}
								</div>
							</div>
						</div>
						<button
							type="button"
							title="Open in Terminal"
							onclick={(e) => {
								e.stopPropagation();
								api.openInTerminal(project.path);
							}}
							class="shrink-0 rounded-full p-2 transition hover:bg-[var(--bg-elevated)]"
							style="color: var(--text-muted);"
						>
							<Terminal class="h-3.5 w-3.5" />
						</button>
					</div>
				{/each}
			</div>
		{/if}
	</section>
</div>

<style>
	.wb-telemetry-grid {
		display: grid;
		grid-template-columns: 1fr;
		gap: 0.75rem;
	}

	@media (min-width: 720px) {
		.wb-telemetry-grid {
			grid-template-columns: repeat(3, minmax(0, 1fr));
		}
	}
</style>
