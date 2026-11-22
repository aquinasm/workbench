<script lang="ts">
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { formatBytes } from '$lib/format';
	import TechIcon from '$lib/components/TechIcon.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import {
		Play,
		Square,
		Download,
		Trash2,
		ExternalLink,
		Terminal,
		RefreshCw,
		RotateCw,
		FileCode2,
		X
	} from 'lucide-svelte';

	let selectedLogServiceId = $state<string | null>(null);
	let logSourcePath = $state<string | null>(null);
	let fetchedLogLines = $state<string[]>([]);
	let loadingLogs = $state(false);
	let portDrafts = $state<Record<string, number>>({});
	let bulkBusy = $state(false);

	// Webserver generated config preview (read-only)
	let previewServer = $state<'caddy' | 'apache' | null>(null);
	let previewContent = $state('');

	let logsReqSeq = 0;
	let previewReqSeq = 0;

	async function openServiceLogs(serviceId: string) {
		if (selectedLogServiceId === serviceId) {
			selectedLogServiceId = null;
			return;
		}
		selectedLogServiceId = serviceId;
		await refreshSelectedLogs(serviceId);
	}

	async function refreshSelectedLogs(serviceId: string) {
		const seq = ++logsReqSeq;
		loadingLogs = true;
		try {
			const res = await api.getServiceLogs(serviceId);
			if (seq !== logsReqSeq) return;
			logSourcePath = res.source_path;
			fetchedLogLines = res.lines;
		} catch (e) {
			if (seq !== logsReqSeq) return;
			wb.showToast(`Failed loading logs: ${e}`, 'error');
		} finally {
			if (seq === logsReqSeq) {
				loadingLogs = false;
			}
		}
	}

	const runningCount = $derived(
		wb.services.filter((s) => s.status === 'running' || s.status === 'external_running').length
	);

	const activeLogService = $derived(
		wb.services.find((s) => s.id === selectedLogServiceId) ?? null
	);

	const combinedLogLines = $derived.by(() => {
		if (!activeLogService) return [];
		const live = wb.serviceLogs[activeLogService.id] ?? [];
		if (fetchedLogLines.length === 0) return live.length > 0 ? live : activeLogService.logs;
		if (live.length === 0) return fetchedLogLines;
		return [...fetchedLogLines, ...live].slice(-300);
	});

	function getPort(id: string, fallback: number): number {
		return portDrafts[id] ?? fallback;
	}

	async function openConfigPreview(server: 'caddy' | 'apache') {
		if (previewServer === server) {
			previewServer = null;
			return;
		}
		const seq = ++previewReqSeq;
		previewServer = server;
		try {
			const content = await api.previewWebserverConfig(server);
			if (seq !== previewReqSeq) return;
			previewContent = content;
		} catch (e) {
			if (seq !== previewReqSeq) return;
			previewContent = `Error generating config: ${e}`;
		}
	}
</script>

<div class="mx-auto max-w-6xl space-y-6">
	<header class="flex flex-wrap items-end justify-between gap-3 border-b pb-4" style="border-color: var(--border-subtle);">
		<div>
			<h1 class="text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
				Services
			</h1>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			<button
				type="button"
				disabled={bulkBusy}
				onclick={async () => {
					bulkBusy = true;
					await wb.startAllServices();
					bulkBusy = false;
				}}
				class="wb-btn-primary"
			>
				<Play class="h-3.5 w-3.5" />
				Start All
			</button>
			<button
				type="button"
				disabled={bulkBusy || runningCount === 0}
				onclick={async () => {
					bulkBusy = true;
					await wb.stopAllServices();
					bulkBusy = false;
				}}
				class="wb-btn-danger"
			>
				<Square class="h-3.5 w-3.5" />
				Stop All ({runningCount})
			</button>
			<button
				type="button"
				disabled={wb.loadingServices}
				onclick={() => wb.refreshServices()}
				class="wb-btn"
			>
				<RefreshCw class="h-3.5 w-3.5 {wb.loadingServices ? 'animate-spin' : ''}" />
				Refresh
			</button>
		</div>
	</header>

	<!-- Services Cards -->
	<section class="wb-grid-services">
		{#each wb.services as svc (svc.id)}
			{@const isRunning = svc.status === 'running' || svc.status === 'external_running'}
			{@const isExtRunning = svc.status === 'external_running'}
			{@const prog = wb.serviceProgress[svc.id]}
			{@const canRun = svc.installed_managed || (svc.id !== 'phpmyadmin' && !!svc.host_binary)}
			<div
				class="flex flex-col justify-between rounded-[14px] p-4 space-y-4"
				style="background-color: var(--bg-surface); box-shadow: var(--shadow-card);"
			>
				<div class="flex items-start justify-between gap-3">
					<div class="flex items-start gap-3 min-w-0">
						<div
							class="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-[10px]"
							style="background-color: var(--bg-elevated);"
						>
							<TechIcon name={svc.id} class="h-5 w-5" />
						</div>
						<div class="min-w-0">
							<div class="flex flex-wrap items-center gap-1.5">
								<span class="text-sm font-semibold" style="color: var(--text-primary);">
									{svc.name}
								</span>
								<span
									class="rounded-full px-2 py-0.5 font-mono text-[10.5px]"
									style="background-color: var(--bg-elevated); color: var(--text-secondary);"
								>
									v{svc.version}
								</span>
								{#if !svc.installed_managed && svc.host_binary && svc.id !== 'phpmyadmin'}
									<span
										class="rounded-full px-2 py-0.5 font-mono text-[10px] font-medium uppercase"
										style="background-color: var(--bg-elevated); color: var(--text-muted);"
									>
										{svc.host_source ?? 'External'}
									</span>
								{:else if svc.installed_managed}
									<span
										class="rounded-full px-2 py-0.5 font-mono text-[10px] font-medium uppercase"
										style="background-color: var(--accent-soft); color: var(--accent);"
									>
										Managed
									</span>
								{/if}
							</div>
							<p class="mt-1 text-xs" style="color: var(--text-secondary);">
								{svc.description}
							</p>
							<div class="mt-1 flex items-center gap-1.5 text-[11px] min-w-0" style="color: var(--text-muted);">
								{#if svc.installed_managed}
									<span class="truncate">Managed ({formatBytes(svc.disk_bytes)})</span>
								{:else if svc.host_binary}
									<span class="truncate font-mono" title={svc.host_binary}>{svc.host_binary}</span>
								{:else}
									<span>Not installed</span>
								{/if}
								{#if isRunning && svc.pid}
									<span class="shrink-0 font-mono">· PID {svc.pid}</span>
								{/if}
							</div>
						</div>
					</div>

					<span
						class="inline-flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 font-mono text-[11px] font-medium"
						style="
							background-color: {isRunning ? 'color-mix(in srgb, var(--success) 14%, transparent)' : 'var(--bg-elevated)'};
							color: {isRunning ? 'var(--success)' : 'var(--text-muted)'};
						"
					>
						<span
							class="h-1.5 w-1.5 rounded-full {isRunning ? 'animate-pulse' : ''}"
							style="background-color: {isRunning ? 'var(--success)' : 'var(--text-muted)'};"
						></span>
						{isExtRunning ? 'Running (Ext)' : isRunning ? 'Running' : 'Stopped'}
					</span>
				</div>

				{#if prog}
					<div class="space-y-1">
						<div class="flex justify-between text-[11px]" style="color: var(--accent);">
							<span>{prog.stage === 'extracting' ? 'Extracting...' : 'Downloading...'}</span>
							<span class="tabular-nums">{prog.percent}%</span>
						</div>
						<div class="h-1.5 w-full overflow-hidden rounded-full" style="background-color: var(--bg-app);">
							<div
								class="h-full rounded-full transition-all"
								style="width: {prog.percent}%; background-color: var(--accent);"
							></div>
						</div>
					</div>
				{/if}

				<div class="flex flex-wrap items-center justify-between gap-2 pt-0.5">
					<label class="inline-flex items-center gap-2 text-xs" style="color: var(--text-muted);">
						<span>Port</span>
						<input
							type="number"
							min="1024"
							max="65535"
							disabled={isRunning}
							value={getPort(svc.id, svc.port)}
							onchange={(e) => {
								portDrafts = {
									...portDrafts,
									[svc.id]: Number((e.currentTarget as HTMLInputElement).value) || svc.port
								};
							}}
							class="w-20 px-2.5 py-1 font-mono text-xs disabled:opacity-50"
							style="min-height: 30px; color: var(--text-primary);"
						/>
					</label>

					<div class="flex flex-wrap items-center gap-1.5">
						{#if svc.id === 'caddy' || svc.id === 'apache'}
							<button
								type="button"
								title="Preview generated {svc.id === 'caddy' ? 'Caddyfile' : 'Apache vhosts'}"
								onclick={() => openConfigPreview(svc.id as 'caddy' | 'apache')}
								class="wb-btn px-2.5"
								style="
									background-color: {previewServer === svc.id ? 'var(--accent-soft)' : ''};
									color: {previewServer === svc.id ? 'var(--accent)' : ''};
								"
							>
								<FileCode2 class="h-3.5 w-3.5" />
							</button>
						{/if}

						{#if isRunning && svc.url}
							<button
								type="button"
								title="Open {svc.url}"
								onclick={() => api.openExternalUrl(svc.url!)}
								class="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium transition hover:opacity-85"
								style="background-color: var(--accent-soft); color: var(--accent);"
							>
								<ExternalLink class="h-3.5 w-3.5" />
								Open
							</button>
						{/if}

						<button
							type="button"
							title="View service logs"
							onclick={() => openServiceLogs(svc.id)}
							class="wb-btn px-2.5"
							style="background-color: {selectedLogServiceId === svc.id ? 'var(--accent-soft)' : ''}; color: {selectedLogServiceId === svc.id ? 'var(--accent)' : ''};"
						>
							<Terminal class="h-3.5 w-3.5" />
						</button>

						{#if !svc.installed_managed && svc.downloadable && !prog}
							<button
								type="button"
								onclick={() => wb.installService(svc.id)}
								class="wb-btn"
							>
								<Download class="h-3.5 w-3.5" />
								Install (~{svc.size_hint_mb} MB)
							</button>
						{/if}

						{#if svc.installed_managed && !isRunning && !prog}
							<button
								type="button"
								title="Uninstall managed {svc.name} binaries (preserves data)"
								onclick={() => wb.uninstallService(svc.id)}
								class="wb-btn px-2.5 hover:text-[var(--danger)]"
							>
								<Trash2 class="h-3.5 w-3.5" />
							</button>
						{/if}

						{#if isRunning}
							<button
								type="button"
								title="Gracefully restart {svc.name}"
								onclick={() => wb.restartService(svc.id, getPort(svc.id, svc.port))}
								class="wb-btn px-2.5"
							>
								<RotateCw class="h-3.5 w-3.5" />
							</button>
							<button
								type="button"
								onclick={() => wb.stopService(svc.id)}
								class="wb-btn-danger"
							>
								<Square class="h-3 w-3" />
								Stop
							</button>
						{:else}
							<button
								type="button"
								disabled={!canRun || !!prog}
								onclick={() => wb.startService(svc.id, getPort(svc.id, svc.port))}
								class="wb-btn-primary"
							>
								<Play class="h-3 w-3" />
								Start
							</button>
						{/if}
					</div>
				</div>
			</div>
		{/each}
	</section>

	<!-- Read-only Generated Web Server Config Preview -->
	{#if previewServer}
		<section
			class="rounded-[14px] p-4 space-y-3"
			style="background-color: var(--bg-surface); box-shadow: var(--shadow-card);"
		>
			<div class="flex items-center justify-between gap-2">
				<span class="wb-kicker">
					Generated {previewServer === 'caddy' ? 'Caddyfile' : 'Apache Virtual Hosts'} (Managed via Projects)
				</span>
				<button
					type="button"
					onclick={() => (previewServer = null)}
					class="rounded-full p-1 transition hover:bg-[var(--bg-surface-hover)]"
					style="color: var(--text-muted);"
				>
					<X class="h-4 w-4" />
				</button>
			</div>
			<div
				class="selectable overflow-x-auto rounded-[10px] p-3.5 font-mono text-[11.5px] whitespace-pre"
				style="background-color: var(--bg-app); color: var(--text-primary);"
			>
				{previewContent}
			</div>
		</section>
	{/if}

	<!-- Colored Live & File-Tailed Service Logs -->
	{#if activeLogService}
		<LogViewer
			title="{activeLogService.name} Logs"
			lines={combinedLogLines}
			sourcePath={logSourcePath}
			loading={loadingLogs}
			onRefresh={() => refreshSelectedLogs(activeLogService.id)}
			onClose={() => (selectedLogServiceId = null)}
		/>
	{/if}
</div>
