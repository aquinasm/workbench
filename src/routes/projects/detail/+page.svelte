<script lang="ts">
	import { page } from '$app/stores';
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { formatBytes, formatIdleDays, kindBadgeClass, kindLabel } from '$lib/format';
	import TechIcon from '$lib/components/TechIcon.svelte';
	import type { LocalSite, ProjectInfo } from '$lib/types';
	import {
		ArrowLeft,
		Terminal,
		FolderOpen,
		Play,
		Snowflake,
		Trash2,
		ShieldCheck,
		Wifi,
		ExternalLink
	} from 'lucide-svelte';

	const projectPath = $derived($page.url.searchParams.get('path') ?? '');
	const project = $derived<ProjectInfo | null>(
		wb.projects.find((p) => p.path === projectPath) ?? null
	);
	const linkedSite = $derived<LocalSite | null>(
		project ? (wb.config.sites ?? []).find((s) => s.project_path === project.path) ?? null : null
	);

	let domainInput = $state('');
	let serverInput = $state<'caddy' | 'apache'>('caddy');
	let modeInput = $state<'php' | 'proxy' | 'static'>('php');
	let proxyPortInput = $state(5173);
	let docrootInput = $state('public');
	let httpsInput = $state(false);
	let exposeLanInput = $state(false);
	let busy = $state(false);

	function stripTestSuffix(raw: string): string {
		return raw
			.trim()
			.toLowerCase()
			.replace(/\.test$/i, '')
			.replace(/[^a-z0-9.-]+/g, '-')
			.replace(/^[.-]+|[.-]+$/g, '');
	}

	const resolvedDomain = $derived.by(() => {
		const base = stripTestSuffix(domainInput);
		return base ? `${base}.test` : '';
	});

	const portless = $derived(wb.networkStatus?.portless_enabled ?? false);
	const httpPort = $derived(
		serverInput === 'apache' && !portless
			? (wb.networkStatus?.apache_port ?? 8081)
			: (wb.networkStatus?.caddy_port ?? 8080)
	);
	const httpsPort = $derived(wb.networkStatus?.caddy_https_port ?? 8443);

	const localPreviewUrl = $derived.by(() => {
		if (!resolvedDomain) return '';
		if (portless) {
			return httpsInput ? `https://${resolvedDomain}` : `http://${resolvedDomain}`;
		}
		if (httpsInput) {
			return httpsPort === 443
				? `https://${resolvedDomain}`
				: `https://${resolvedDomain}:${httpsPort}`;
		}
		return httpPort === 80
			? `http://${resolvedDomain}`
			: `http://${resolvedDomain}:${httpPort}`;
	});

	const lanPreviewUrl = $derived.by(() => {
		const ip = wb.networkStatus?.lan_ip;
		if (!ip || !exposeLanInput) return '';
		return httpPort === 80 ? `http://${ip}` : `http://${ip}:${httpPort}`;
	});

	let hydratedFormKey = '';

	$effect(() => {
		if (!project) return;
		const key = `${project.path}::${JSON.stringify(linkedSite ?? null)}`;
		if (key === hydratedFormKey) return;
		hydratedFormKey = key;
		if (linkedSite) {
			domainInput = stripTestSuffix(linkedSite.domain);
			serverInput = linkedSite.server === 'apache' ? 'apache' : 'caddy';
			modeInput =
				linkedSite.mode === 'proxy'
					? 'proxy'
					: linkedSite.mode === 'static'
						? 'static'
						: 'php';
			proxyPortInput = linkedSite.proxy_port ?? 5173;
			docrootInput = linkedSite.docroot_subpath ?? 'public';
			httpsInput = linkedSite.https ?? false;
			exposeLanInput = linkedSite.expose_lan ?? false;
		} else {
			const slug = project.name
				.toLowerCase()
				.replace(/[^a-z0-9-]+/g, '-')
				.replace(/^-|-$/g, '');
			domainInput = slug || 'app';
			serverInput = 'caddy';
			modeInput =
				project.kind === 'laravel' || project.kind === 'php'
					? 'php'
					: project.kind === 'sveltekit' || project.kind === 'node'
						? 'proxy'
						: 'static';
			proxyPortInput = 5173;
			docrootInput = project.kind === 'laravel' ? 'public' : '';
			httpsInput = false;
			exposeLanInput = false;
		}
	});

	function relevantToolsFor(p: ProjectInfo): string[] {
		const tools = new Set<string>();
		for (const k of [p.kind, ...p.secondary_kinds]) {
			if (k === 'laravel' || k === 'php') {
				tools.add('php');
				tools.add('composer');
				tools.add('node');
			} else if (k === 'sveltekit' || k === 'node') {
				tools.add('node');
			} else if (k === 'flutter') {
				tools.add('flutter');
			} else if (k === 'rust') {
				tools.add('rust');
			}
		}
		return Array.from(tools);
	}

	async function saveSiteLink() {
		if (!project || !resolvedDomain) return;
		if (linkedSite && linkedSite.domain !== resolvedDomain) {
			await wb.removeLocalSite(linkedSite.domain);
		}
		await wb.addLocalSite({
			domain: resolvedDomain,
			project_path: project.path,
			server: serverInput,
			mode: modeInput,
			proxy_port: modeInput === 'proxy' ? proxyPortInput : null,
			docroot_subpath: modeInput === 'php' ? docrootInput.trim() || null : null,
			https: serverInput === 'caddy' ? httpsInput : false,
			expose_lan: exposeLanInput
		});
		await wb.refreshNetworkStatus();
	}

	async function hibernateArtifact(relPath?: string) {
		if (!project) return;
		busy = true;
		await wb.hibernateOne(project.path, relPath ? [relPath] : undefined);
		busy = false;
	}
</script>

<div class="mx-auto max-w-6xl space-y-6">
	<div>
		<a
			href="/projects"
			class="inline-flex items-center gap-1.5 font-mono text-[11px] font-medium transition hover:text-[var(--text-primary)]"
			style="color: var(--text-muted);"
		>
			<ArrowLeft class="h-3.5 w-3.5" />
			Back to Projects
		</a>
	</div>

	{#if !project}
		<div
			class="border p-10 text-center space-y-2"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			<div class="text-sm font-semibold" style="color: var(--text-primary);">
				Project not found
			</div>
			<p class="font-mono text-xs" style="color: var(--text-muted);">
				{projectPath || 'No project path specified.'}
			</p>
		</div>
	{:else}
		<!-- Header -->
		<header
			class="flex flex-wrap items-end justify-between gap-4 border-b pb-4"
			style="border-color: var(--border-subtle);"
		>
			<div class="min-w-0">
				<div class="flex items-center gap-2">
					<span style="color: var(--accent);">
						<TechIcon name={project.kind} class="h-4 w-4" />
					</span>
					<span class="wb-kicker">Project Configuration</span>
					<span
						class="rounded-full px-2.5 py-0.5 font-mono text-[10px] font-medium {kindBadgeClass(
							project.kind
						)}"
					>
						{kindLabel(project.kind)}
					</span>
					{#each project.secondary_kinds as sk}
						<span class="rounded-full px-2.5 py-0.5 font-mono text-[10px] {kindBadgeClass(sk)}">
							{kindLabel(sk)}
						</span>
					{/each}
				</div>
				<h1 class="mt-1 truncate text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
					{project.name}
				</h1>
				<div class="selectable mt-0.5 truncate font-mono text-[11px]" style="color: var(--text-muted);">
					{project.path} · {formatIdleDays(project.idle_days)}
				</div>
			</div>

			<div class="flex flex-wrap items-center gap-2">
				<button type="button" onclick={() => api.openInTerminal(project.path)} class="wb-btn">
					<Terminal class="h-3.5 w-3.5" />
					Terminal
				</button>
				<button type="button" onclick={() => api.revealInFileManager(project.path)} class="wb-btn">
					<FolderOpen class="h-3.5 w-3.5" />
					Reveal
				</button>
				{#if !project.hibernated}
					<button type="button" disabled={busy} onclick={() => hibernateArtifact()} class="wb-btn">
						<Snowflake class="h-3.5 w-3.5" />
						Hibernate ({formatBytes(project.total_reclaimable_bytes)})
					</button>
				{/if}
				{#if project.wake_commands.length > 0}
					<button
						type="button"
						disabled={wb.terminalRunning}
						onclick={() => wb.wakeOne(project.path)}
						class="wb-btn-primary"
					>
						<Play class="h-3.5 w-3.5" />
						{project.hibernated ? 'Wake Project' : 'Re-install Deps'}
					</button>
				{/if}
			</div>
		</header>

		<div class="grid grid-cols-1 gap-5 lg:grid-cols-12">
			<!-- Left 7 cols: Runtime Versions + Local Domain -->
			<div class="space-y-5 lg:col-span-7">
				<!-- Runtime Versions -->
				<section
					class="border p-5 space-y-4"
					style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
				>
					<div class="flex items-center justify-between gap-2">
						<div>
							<div class="wb-kicker">Runtime Versions (.workbench.toml)</div>
							<p class="mt-0.5 text-xs" style="color: var(--text-secondary);">
								Pin specific tool versions for this repository or inherit global defaults.
							</p>
						</div>
					</div>

					<div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
						{#each relevantToolsFor(project) as tool}
							{@const installedVersions =
								tool === 'rust'
									? wb.rustup.toolchains.map((t) => ({ version: t.name, label: t.name }))
									: wb.runtimes
											.filter((r) => r.tool === tool && r.installed)
											.map((r) => ({
												version: r.version,
												label: r.is_external
													? `${r.version.replace(/^ext-/, '')} (External · ${r.source ?? 'System'})`
													: `v${r.version}`
											}))}
							<label class="space-y-1.5">
								<span class="block font-mono text-[10.5px] font-medium uppercase" style="color: var(--text-secondary);">
									{tool}
								</span>
								<select
									value={project.version_hints[tool] ?? ''}
									onchange={(e) => {
										const val = (e.currentTarget as HTMLSelectElement).value;
										wb.setProjectPin(project.path, tool, val || null);
									}}
									class="w-full border px-2.5 py-1.5 font-mono text-xs"
									style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
								>
									<option value="">Global Default</option>
									{#each installedVersions as iv}
										<option value={iv.version}>{iv.label}</option>
									{/each}
									{#if project.version_hints[tool] && !installedVersions.some((iv) => iv.version === project.version_hints[tool])}
										<option value={project.version_hints[tool]}>
											{project.version_hints[tool]}
										</option>
									{/if}
								</select>
							</label>
						{/each}
					</div>

					{#if project.wake_commands.length > 0}
						<div class="border-t pt-3 font-mono text-[11px]" style="border-color: var(--border-subtle); color: var(--text-muted);">
							Wake pipeline: <span style="color: var(--text-secondary);">{project.wake_commands.join(' && ')}</span>
						</div>
					{/if}
				</section>

				<!-- Local Domain & Web Server -->
				{#if project.kind !== 'flutter'}
					<section
						class="border p-5 space-y-4"
						style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
					>
						<div class="flex items-center justify-between">
							<div>
								<div class="wb-kicker">Local Domain & Web Server</div>
								<p class="mt-0.5 text-xs" style="color: var(--text-secondary);">
									Configure Caddy or Apache virtual host routing for this project.
								</p>
							</div>
							{#if linkedSite}
								<button
									type="button"
									onclick={() => wb.removeLocalSite(linkedSite.domain)}
									class="wb-btn-danger"
								>
									<Trash2 class="h-3 w-3" />
									Unlink
								</button>
							{/if}
						</div>

						<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
							<label class="space-y-1.5">
								<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">
									Custom Domain
								</span>
								<div
									class="flex items-center overflow-hidden rounded-[10px]"
									style="background-color: var(--bg-elevated);"
								>
									<input
										type="text"
										bind:value={domainInput}
										placeholder="myapp"
										class="wb-bare-input min-w-0 flex-1 px-3 py-2 font-mono text-xs"
										style="color: var(--text-primary);"
									/>
									<span
										class="shrink-0 px-3 py-2 font-mono text-xs font-semibold select-none"
										style="background-color: var(--accent-soft); color: var(--accent);"
									>
										.test
									</span>
								</div>
							</label>

							<label class="space-y-1.5">
								<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">
									Server
								</span>
								<select
									bind:value={serverInput}
									class="w-full border px-2.5 py-1.5 text-xs"
									style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
								>
									<option value="caddy">Caddy</option>
									<option value="apache">Apache (.htaccess)</option>
								</select>
							</label>

							<label class="space-y-1.5">
								<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">
									Serving Mode
								</span>
								<select
									bind:value={modeInput}
									class="w-full border px-2.5 py-1.5 text-xs"
									style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
								>
									<option value="php">PHP / Laravel</option>
									<option value="proxy">Reverse Proxy (SvelteKit / Vite)</option>
									<option value="static">Static Files</option>
								</select>
							</label>

							{#if modeInput === 'proxy'}
								<label class="space-y-1.5">
									<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">
										Dev Port
									</span>
									<input
										type="number"
										bind:value={proxyPortInput}
										class="w-full border px-2.5 py-1.5 font-mono text-xs"
										style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
									/>
								</label>
							{:else if modeInput === 'php'}
								<label class="space-y-1.5">
									<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">
										Docroot Subfolder
									</span>
									<input
										type="text"
										bind:value={docrootInput}
										placeholder="public"
										class="w-full border px-2.5 py-1.5 font-mono text-xs"
										style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
									/>
								</label>
							{/if}
						</div>

						<!-- SSL, LAN Exposure & DNS Controls -->
						<div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
							<label
								class="flex items-center justify-between gap-3 rounded-[10px] px-3 py-2.5 cursor-pointer"
								style="background-color: var(--bg-elevated);"
							>
								<span class="flex items-center gap-2 min-w-0">
									<ShieldCheck class="h-4 w-4 shrink-0" style="color: {httpsInput ? 'var(--accent)' : 'var(--text-muted)'};" />
									<span class="min-w-0">
										<span class="block text-xs font-medium" style="color: var(--text-primary);">
											Local HTTPS (SSL)
										</span>
										<span class="block text-[11px]" style="color: var(--text-muted);">
											{serverInput === 'caddy'
												? 'Auto cert via Caddy internal CA'
												: 'Auto TLS via Caddy front-door proxy'}
										</span>
									</span>
								</span>
								<input
									type="checkbox"
									bind:checked={httpsInput}
									class="h-4 w-4 shrink-0 cursor-pointer"
								/>
							</label>

							<label
								class="flex items-center justify-between gap-3 rounded-[10px] px-3 py-2.5 cursor-pointer"
								style="background-color: var(--bg-elevated);"
							>
								<span class="flex items-center gap-2 min-w-0">
									<Wifi class="h-4 w-4 shrink-0" style="color: {exposeLanInput ? 'var(--accent)' : 'var(--text-muted)'};" />
									<span class="min-w-0">
										<span class="block text-xs font-medium" style="color: var(--text-primary);">
											Expose on Local Network
										</span>
										<span class="block truncate font-mono text-[11px]" style="color: var(--text-muted);">
											{wb.networkStatus?.lan_ip ?? 'No LAN IP detected'}
										</span>
									</span>
								</span>
								<input
									type="checkbox"
									bind:checked={exposeLanInput}
									class="h-4 w-4 shrink-0 cursor-pointer"
								/>
							</label>
						</div>

						<div class="flex flex-wrap items-center justify-between gap-3 pt-1">
							<div class="flex flex-wrap items-center gap-3 min-w-0">
								{#if localPreviewUrl}
									<button
										type="button"
										onclick={() => api.openExternalUrl(localPreviewUrl)}
										class="inline-flex items-center gap-1.5 truncate font-mono text-xs hover:underline"
										style="color: var(--accent);"
									>
										<span class="truncate">{localPreviewUrl}</span>
										<ExternalLink class="h-3 w-3 shrink-0" />
									</button>
								{/if}
								{#if lanPreviewUrl}
									<button
										type="button"
										onclick={() => api.openExternalUrl(lanPreviewUrl)}
										class="inline-flex items-center gap-1 truncate font-mono text-xs hover:underline"
										style="color: var(--text-secondary);"
									>
										<span>LAN: {lanPreviewUrl}</span>
										<ExternalLink class="h-3 w-3 shrink-0" />
									</button>
								{/if}
							</div>
							<button
								type="button"
								disabled={!resolvedDomain}
								onclick={saveSiteLink}
								class="wb-btn-primary shrink-0"
							>
								{linkedSite ? 'Update Site' : 'Link Local Site'}
							</button>
						</div>
					</section>
				{/if}
			</div>

			<!-- Right 5 cols: Storage & Artifacts Breakdown -->
			<section
				class="border p-5 space-y-4 lg:col-span-5 h-fit"
				style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
			>
				<div class="flex items-center justify-between">
					<div class="wb-kicker">Regenerable Artifacts</div>
					<span class="font-mono text-sm font-bold tabular-nums" style="color: var(--text-primary);">
						{formatBytes(project.total_reclaimable_bytes)}
					</span>
				</div>

				{#if project.artifacts.length === 0}
					<div
						class="border p-6 text-center font-mono text-xs"
						style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-muted);"
					>
						HIBERNATED — 0 B RECLAIMABLE
					</div>
				{:else}
					<div class="divide-y overflow-hidden rounded-[10px] border" style="border-color: var(--border-subtle); background-color: var(--bg-app);">
						{#each project.artifacts as art (art.rel_path)}
							<div class="flex items-center justify-between gap-2 px-3 py-2.5 text-xs">
								<div class="min-w-0">
									<div class="truncate font-mono font-medium" style="color: var(--text-primary);">
										{art.rel_path}
									</div>
									<div class="text-[11px]" style="color: var(--text-muted);">{art.label}</div>
								</div>
								<div class="flex shrink-0 items-center gap-2.5">
									<span class="font-mono font-semibold tabular-nums" style="color: var(--text-primary);">
										{formatBytes(art.size_bytes)}
									</span>
									<button
										type="button"
										disabled={busy}
										title="Delete {art.rel_path}"
										onclick={() => hibernateArtifact(art.rel_path)}
										class="p-1 transition hover:opacity-75"
										style="color: var(--danger);"
									>
										<Trash2 class="h-3.5 w-3.5" />
									</button>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</section>
		</div>
	{/if}
</div>
