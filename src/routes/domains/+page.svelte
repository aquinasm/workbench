<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { buildSiteUrl } from '$lib/format';
	import type { LocalSite } from '$lib/types';
	import {
		Globe,
		Plus,
		Search,
		ShieldCheck,
		ExternalLink,
		Copy,
		Check,
		Pencil,
		Trash2,
		FolderOpen,
		Wifi,
		RefreshCw,
		X,
		AlertCircle
	} from 'lucide-svelte';

	const RESERVED_DOMAINS = new Set([
		'phpmyadmin.test',
		'pma.test',
		'localhost.test',
		'workbench.test'
	]);
	const RESERVED_PROXY_PORTS = new Set([8080, 8443, 8081, 8088, 3306, 6379, 15353]);

	let searchQuery = $state('');
	let serverFilter = $state<'all' | 'caddy' | 'apache' | 'https'>('all');
	let copiedDomain = $state<string | null>(null);

	// Add / Edit form state
	let formOpen = $state(false);
	let editingOriginalDomain = $state<string | null>(null);
	let domainInput = $state('');
	let projectPathInput = $state('');
	let serverInput = $state<'caddy' | 'apache'>('caddy');
	let modeInput = $state<'php' | 'proxy' | 'static'>('php');
	let docrootInput = $state('public');
	let proxyPortInput = $state(5173);
	let httpsInput = $state(true);
	let exposeLanInput = $state(false);
	let saving = $state(false);

	onMount(() => {
		void wb.refreshNetworkStatus();
	});

	function normalizeBase(raw: string): string {
		return raw
			.trim()
			.toLowerCase()
			.replace(/^https?:\/\//, '')
			.replace(/\/.*$/, '')
			.replace(/\.test$/i, '')
			.replace(/[^a-z0-9.-]+/g, '-')
			.replace(/^[.-]+|[.-]+$/g, '');
	}

	const resolvedDomain = $derived.by(() => {
		const base = normalizeBase(domainInput);
		return base ? `${base}.test` : '';
	});

	const sites = $derived<LocalSite[]>(wb.config.sites ?? []);

	const validationError = $derived.by(() => {
		if (!domainInput.trim()) return null;
		const dom = resolvedDomain;
		if (!dom) return 'Enter a valid domain name (letters, numbers, hyphens).';
		if (dom.length > 253) return 'Domain exceeds 253 characters.';
		for (const label of dom.split('.')) {
			if (!label || label.length > 63 || label.startsWith('-') || label.endsWith('-')) {
				return 'Domain labels cannot be empty or start/end with a hyphen.';
			}
		}
		if (RESERVED_DOMAINS.has(dom)) {
			return `'${dom}' is a reserved system domain.`;
		}
		const conflict = sites.find(
			(s) =>
				s.domain.toLowerCase() === dom &&
				s.domain.toLowerCase() !== (editingOriginalDomain ?? '').toLowerCase()
		);
		if (conflict) {
			return `'${dom}' is already linked to ${conflict.project_path}.`;
		}
		if (modeInput === 'proxy') {
			const p = Number(proxyPortInput);
			if (!Number.isInteger(p) || p < 1024 || p > 65535) {
				return 'Proxy port must be between 1024 and 65535.';
			}
			if (RESERVED_PROXY_PORTS.has(p)) {
				return `Port ${p} conflicts with a Workbench internal service port.`;
			}
		}
		return null;
	});

	const canSubmit = $derived(
		!!resolvedDomain && !!projectPathInput.trim() && !validationError && !saving
	);

	const filteredSites = $derived.by(() => {
		const q = searchQuery.trim().toLowerCase();
		return sites
			.filter((s) => {
				if (serverFilter === 'caddy' && s.server !== 'caddy') return false;
				if (serverFilter === 'apache' && s.server !== 'apache') return false;
				if (serverFilter === 'https' && !s.https) return false;
				if (q) {
					return (
						s.domain.toLowerCase().includes(q) ||
						s.project_path.toLowerCase().includes(q)
					);
				}
				return true;
			})
			.sort((a, b) => a.domain.localeCompare(b.domain));
	});

	const pmaUrl = $derived.by(() => {
		if (wb.networkStatus?.portless_enabled) {
			return 'https://phpmyadmin.test';
		}
		const pmaSvc = wb.services.find((s) => s.id === 'phpmyadmin');
		return pmaSvc?.url ?? 'http://127.0.0.1:8088';
	});

	function openAddForm() {
		editingOriginalDomain = null;
		domainInput = '';
		projectPathInput = wb.projects[0]?.path ?? '';
		serverInput = 'caddy';
		modeInput = 'php';
		docrootInput = 'public';
		proxyPortInput = 5173;
		httpsInput = true;
		exposeLanInput = false;
		formOpen = true;
	}

	function openEditForm(site: LocalSite) {
		editingOriginalDomain = site.domain;
		domainInput = normalizeBase(site.domain);
		projectPathInput = site.project_path;
		serverInput = site.server === 'apache' ? 'apache' : 'caddy';
		modeInput = site.mode === 'proxy' ? 'proxy' : site.mode === 'static' ? 'static' : 'php';
		docrootInput = site.docroot_subpath ?? '';
		proxyPortInput = site.proxy_port ?? 5173;
		httpsInput = site.https ?? false;
		exposeLanInput = site.expose_lan ?? false;
		formOpen = true;
	}

	function onProjectSelected(path: string) {
		projectPathInput = path;
		if (!editingOriginalDomain && !domainInput.trim()) {
			const proj = wb.projects.find((p) => p.path === path);
			if (proj) {
				domainInput = normalizeBase(proj.name);
				if (proj.kind === 'laravel') {
					modeInput = 'php';
					docrootInput = 'public';
				} else if (proj.kind === 'php') {
					modeInput = 'php';
					docrootInput = '';
				} else if (proj.kind === 'sveltekit' || proj.kind === 'node') {
					modeInput = 'proxy';
					proxyPortInput = 5173;
				}
			}
		}
	}

	async function browseProjectFolder() {
		const picked = await api.pickFolder();
		if (picked) {
			projectPathInput = picked;
			if (!editingOriginalDomain && !domainInput.trim()) {
				const leaf = picked.split(/[\\/]/).filter(Boolean).pop() ?? '';
				domainInput = normalizeBase(leaf);
			}
		}
	}

	async function handleSaveDomain(e: SubmitEvent) {
		e.preventDefault();
		if (!canSubmit) return;
		saving = true;
		try {
			// If renaming an existing domain, remove the old entry first
			if (
				editingOriginalDomain &&
				editingOriginalDomain.toLowerCase() !== resolvedDomain.toLowerCase()
			) {
				await wb.removeLocalSite(editingOriginalDomain);
			}
			await wb.addLocalSite({
				domain: resolvedDomain,
				project_path: projectPathInput.trim(),
				server: serverInput,
				mode: modeInput,
				proxy_port: modeInput === 'proxy' ? Number(proxyPortInput) : null,
				docroot_subpath:
					modeInput !== 'proxy' && docrootInput.trim() ? docrootInput.trim() : null,
				https: httpsInput,
				expose_lan: exposeLanInput
			});
			formOpen = false;
			editingOriginalDomain = null;
		} finally {
			saving = false;
		}
	}

	async function copyUrl(domain: string, url: string) {
		try {
			await navigator.clipboard.writeText(url);
			copiedDomain = domain;
			setTimeout(() => {
				if (copiedDomain === domain) copiedDomain = null;
			}, 1500);
		} catch {
			// ignore clipboard failures
		}
	}
</script>

<div class="mx-auto w-full max-w-[min(1680px,100%)] space-y-6">
	<!-- Sticky Page Header -->
	<header class="wb-page-header">
		<div>
			<h1 class="text-2xl font-semibold tracking-tight" style="color: var(--text-primary);">
				Domains
			</h1>
			<p class="mt-0.5 text-xs" style="color: var(--text-secondary);">
				Manage local <span class="font-mono">.test</span> domains across Caddy and Apache simultaneously.
			</p>
		</div>

		<div class="flex flex-wrap items-center gap-2">
			<button
				type="button"
				disabled={wb.syncingDns}
				onclick={() => wb.syncHostsDns()}
				class="wb-btn"
				title="Configure .test DNS and local HTTP/HTTPS routing"
			>
				<RefreshCw class="h-3.5 w-3.5 {wb.syncingDns ? 'animate-spin' : ''}" />
				{#if wb.networkStatus?.wildcard_dns_enabled && wb.networkStatus?.portless_enabled}
					Sync DNS
				{:else}
					Setup .test DNS
				{/if}
			</button>
			<button
				type="button"
				disabled={wb.trustingCa}
				onclick={() => wb.trustLocalCa()}
				class="wb-btn"
			>
				<ShieldCheck class="h-3.5 w-3.5" />
				{wb.trustingCa ? 'Trusting CA...' : 'Trust Local CA'}
			</button>
			<button type="button" onclick={openAddForm} class="wb-btn-primary">
				<Plus class="h-3.5 w-3.5" />
				Add Domain
			</button>
		</div>
	</header>

	<!-- Live DNS & Routing Status Banner -->
	{#if wb.networkStatus}
		<section
			class="flex flex-wrap items-center justify-between gap-3 rounded-[16px] px-4 py-3"
			style="background-color: var(--bg-surface); box-shadow: var(--shadow-card);"
		>
			<div class="flex flex-wrap items-center gap-4 text-xs">
				<div class="flex items-center gap-2">
					<span
						class="h-2 w-2 rounded-full"
						style="background-color: {wb.networkStatus.wildcard_dns_enabled
							? 'var(--success)'
							: wb.networkStatus.missing_domains.length === 0
								? 'var(--success)'
								: 'var(--warning)'};"
					></span>
					<span style="color: var(--text-secondary);">DNS:</span>
					<strong class="font-mono" style="color: var(--text-primary);">
						{#if wb.networkStatus.wildcard_dns_enabled}
							*.test
						{:else if wb.networkStatus.missing_domains.length === 0}
							Synced ({wb.networkStatus.synced_domains.length})
						{:else}
							{wb.networkStatus.missing_domains.length} pending
						{/if}
					</strong>
				</div>

				<div class="flex items-center gap-2">
					<span
						class="h-2 w-2 rounded-full"
						style="background-color: {wb.networkStatus.portless_enabled
							? 'var(--success)'
							: 'var(--text-muted)'};"
					></span>
					<span style="color: var(--text-secondary);">Routing:</span>
					<strong class="font-mono" style="color: var(--text-primary);">
						{wb.networkStatus.portless_enabled
							? 'HTTP :80 / HTTPS :443'
							: `:${wb.networkStatus.caddy_port} / :${wb.networkStatus.caddy_https_port}`}
					</strong>
				</div>

				{#if wb.networkStatus.lan_ip}
					<div class="flex items-center gap-1.5 font-mono" style="color: var(--text-muted);">
						<Wifi class="h-3.5 w-3.5" style="color: var(--accent);" />
						{wb.networkStatus.lan_ip}
					</div>
				{/if}
			</div>

			{#if !wb.networkStatus.wildcard_dns_enabled || !wb.networkStatus.portless_enabled}
				<span class="text-[11px]" style="color: var(--text-muted);">
					Run <strong style="color: var(--text-primary);">Setup .test DNS</strong> to enable automatic local domain resolution.
				</span>
			{/if}
		</section>
	{/if}

	<!-- Add / Edit Domain Form -->
	{#if formOpen}
		<form
			onsubmit={handleSaveDomain}
			class="rounded-[18px] p-5 space-y-4"
			style="background-color: var(--bg-surface); box-shadow: var(--shadow-card);"
		>
			<div class="flex items-center justify-between gap-2">
				<div>
					<h2 class="text-sm font-semibold" style="color: var(--text-primary);">
						{editingOriginalDomain ? `Edit ${editingOriginalDomain}` : 'Link New Local Domain'}
					</h2>
					<p class="text-xs" style="color: var(--text-secondary);">
						Assign a local <span class="font-mono">.test</span> domain backed by Caddy or Apache.
					</p>
				</div>
				<button
					type="button"
					onclick={() => (formOpen = false)}
					class="rounded-full p-1.5 transition hover:bg-[var(--bg-surface-hover)]"
					style="color: var(--text-muted);"
				>
					<X class="h-4 w-4" />
				</button>
			</div>

			<div class="grid grid-cols-1 gap-3.5 md:grid-cols-2">
				<!-- Domain Input -->
				<label class="space-y-1.5">
					<span class="wb-kicker block">Domain</span>
					<div
						class="flex items-center overflow-hidden rounded-[10px]"
						style="background-color: var(--bg-elevated);"
					>
						<input
							type="text"
							bind:value={domainInput}
							placeholder="myapp"
							required
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

				<!-- Project Folder Selector -->
				<div class="space-y-1.5">
					<span class="wb-kicker block">Project Directory</span>
					<div class="flex items-center gap-2">
						<select
							value={wb.projects.some((p) => p.path === projectPathInput) ? projectPathInput : ''}
							onchange={(e) => {
								const v = (e.currentTarget as HTMLSelectElement).value;
								if (v) onProjectSelected(v);
							}}
							class="min-w-0 flex-1 rounded-[10px] px-3 py-2 text-xs"
							style="background-color: var(--bg-elevated); color: var(--text-primary);"
						>
							<option value="">Select discovered project...</option>
							{#each wb.projects as p (p.path)}
								<option value={p.path}>{p.name} ({p.kind})</option>
							{/each}
						</select>
						<button type="button" onclick={browseProjectFolder} class="wb-btn shrink-0">
							<FolderOpen class="h-3.5 w-3.5" />
							Browse
						</button>
					</div>
					{#if projectPathInput}
						<div class="truncate font-mono text-[10.5px]" style="color: var(--text-muted);">
							{projectPathInput}
						</div>
					{/if}
				</div>

				<!-- Server Selector -->
				<label class="space-y-1.5">
					<span class="wb-kicker block">Web Server</span>
					<select
						bind:value={serverInput}
						class="w-full rounded-[10px] px-3 py-2 text-xs"
						style="background-color: var(--bg-elevated); color: var(--text-primary);"
					>
						<option value="caddy">Caddy (Fast reverse proxy & native HTTPS)</option>
						<option value="apache">Apache HTTP (.htaccess support, proxied via Caddy)</option>
					</select>
				</label>

				<!-- Serving Mode -->
				<label class="space-y-1.5">
					<span class="wb-kicker block">Serving Mode</span>
					<select
						bind:value={modeInput}
						class="w-full rounded-[10px] px-3 py-2 text-xs"
						style="background-color: var(--bg-elevated); color: var(--text-primary);"
					>
						<option value="php">PHP / Laravel (Per-project PHP worker)</option>
						<option value="proxy">Reverse Proxy (Vite / Node / Next.js port)</option>
						<option value="static">Static Files</option>
					</select>
				</label>

				{#if modeInput === 'proxy'}
					<label class="space-y-1.5">
						<span class="wb-kicker block">Upstream Port</span>
						<input
							type="number"
							min="1024"
							max="65535"
							bind:value={proxyPortInput}
							class="w-full rounded-[10px] px-3 py-2 font-mono text-xs"
							style="background-color: var(--bg-elevated); color: var(--text-primary);"
						/>
					</label>
				{:else}
					<label class="space-y-1.5">
						<span class="wb-kicker block">Docroot Subfolder</span>
						<input
							type="text"
							bind:value={docrootInput}
							placeholder="public (leave empty for project root)"
							class="w-full rounded-[10px] px-3 py-2 font-mono text-xs"
							style="background-color: var(--bg-elevated); color: var(--text-primary);"
						/>
					</label>
				{/if}

				<!-- HTTPS & LAN Toggles -->
				<div class="grid grid-cols-2 gap-2.5 self-end">
					<label
						class="flex cursor-pointer items-center justify-between gap-2 rounded-[10px] px-3 py-2"
						style="background-color: var(--bg-elevated);"
					>
						<span class="text-xs font-medium" style="color: var(--text-primary);">HTTPS (SSL)</span>
						<input type="checkbox" bind:checked={httpsInput} class="h-4 w-4 cursor-pointer" />
					</label>
					<label
						class="flex cursor-pointer items-center justify-between gap-2 rounded-[10px] px-3 py-2"
						style="background-color: var(--bg-elevated);"
					>
						<span class="text-xs font-medium" style="color: var(--text-primary);">LAN Expose</span>
						<input type="checkbox" bind:checked={exposeLanInput} class="h-4 w-4 cursor-pointer" />
					</label>
				</div>
			</div>

			{#if validationError}
				<div
					class="flex items-center gap-2 rounded-[10px] px-3 py-2 text-xs"
					style="background-color: color-mix(in srgb, var(--danger) 12%, transparent); color: var(--danger);"
				>
					<AlertCircle class="h-3.5 w-3.5 shrink-0" />
					<span>{validationError}</span>
				</div>
			{/if}

			<div class="flex items-center justify-between gap-3 pt-1">
				<span class="font-mono text-xs" style="color: var(--accent);">
					{#if resolvedDomain}
						{httpsInput ? 'https' : 'http'}://{resolvedDomain}
					{/if}
				</span>
				<div class="flex items-center gap-2">
					<button type="button" onclick={() => (formOpen = false)} class="wb-btn">Cancel</button>
					<button type="submit" disabled={!canSubmit} class="wb-btn-primary">
						{saving ? 'Saving...' : editingOriginalDomain ? 'Update Domain' : 'Create Domain'}
					</button>
				</div>
			</div>
		</form>
	{/if}

	<!-- Search & Filter Toolbar -->
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div
			class="flex min-w-[240px] flex-1 max-w-md items-center gap-2 rounded-[10px] px-3 py-2"
			style="background-color: var(--bg-surface);"
		>
			<Search class="h-3.5 w-3.5 shrink-0" style="color: var(--text-muted);" />
			<input
				type="text"
				bind:value={searchQuery}
				placeholder="Filter domains or project paths..."
				class="wb-bare-input min-w-0 flex-1 text-xs"
				style="color: var(--text-primary);"
			/>
		</div>

		<div class="flex items-center gap-1.5">
			{#each [
				{ id: 'all', label: `All (${sites.length})` },
				{ id: 'caddy', label: 'Caddy' },
				{ id: 'apache', label: 'Apache' },
				{ id: 'https', label: 'HTTPS' }
			] as tab (tab.id)}
				<button
					type="button"
					onclick={() => (serverFilter = tab.id as 'all' | 'caddy' | 'apache' | 'https')}
					class="rounded-[9px] px-3 py-1.5 text-xs font-medium transition"
					style={serverFilter === tab.id
						? 'background-color: var(--accent-soft); color: var(--accent);'
						: 'background-color: var(--bg-surface); color: var(--text-secondary);'}
				>
					{tab.label}
				</button>
			{/each}
		</div>
	</div>

	<!-- Built-in System Domain (phpMyAdmin) -->
	<section class="space-y-2.5">
		<div class="wb-kicker">System Domain</div>
		<div
			class="flex flex-wrap items-center justify-between gap-3 rounded-[16px] px-4 py-3.5"
			style="background-color: var(--bg-surface); box-shadow: var(--shadow-card);"
		>
			<div class="flex min-w-0 items-center gap-3">
				<div
					class="flex h-9 w-9 shrink-0 items-center justify-center rounded-[10px]"
					style="background-color: var(--accent-soft); color: var(--accent);"
				>
					<Globe class="h-4 w-4" />
				</div>
				<div class="min-w-0">
					<div class="flex flex-wrap items-center gap-2">
						<span class="font-mono text-sm font-semibold" style="color: var(--text-primary);">
							phpmyadmin.test
						</span>
						<span class="wb-pill text-[10px] font-mono">SYSTEM</span>
						<span class="wb-pill text-[10px] font-mono">Caddy → :8088</span>
					</div>
					<div class="mt-0.5 text-xs" style="color: var(--text-secondary);">
						Reserved built-in domain for phpMyAdmin web interface
					</div>
				</div>
			</div>

			<div class="flex items-center gap-2">
				<button
					type="button"
					onclick={() => copyUrl('phpmyadmin.test', pmaUrl)}
					class="wb-btn px-2.5"
					title="Copy URL"
				>
					{#if copiedDomain === 'phpmyadmin.test'}
						<Check class="h-3.5 w-3.5" style="color: var(--success);" />
					{:else}
						<Copy class="h-3.5 w-3.5" />
					{/if}
				</button>
				<button
					type="button"
					onclick={() => api.openExternalUrl(pmaUrl)}
					class="wb-btn"
				>
					<ExternalLink class="h-3.5 w-3.5" />
					Open
				</button>
			</div>
		</div>
	</section>

	<!-- Linked Project Domains -->
	<section class="space-y-2.5">
		<div class="wb-kicker">Project Domains ({filteredSites.length})</div>

		{#if filteredSites.length === 0}
			<div
				class="flex flex-col items-center justify-center rounded-[18px] px-6 py-12 text-center"
				style="background-color: var(--bg-surface); box-shadow: var(--shadow-card);"
			>
				<Globe class="h-8 w-8" style="color: var(--text-muted);" />
				<p class="mt-3 text-sm font-medium" style="color: var(--text-primary);">
					{sites.length === 0 ? 'No custom domains linked yet' : 'No domains match your filter'}
				</p>
				<p class="mt-1 text-xs" style="color: var(--text-secondary);">
					Link any project to a clean <span class="font-mono">.test</span> domain backed by Caddy or Apache.
				</p>
				{#if sites.length === 0}
					<button type="button" onclick={openAddForm} class="wb-btn-primary mt-4">
						<Plus class="h-3.5 w-3.5" />
						Add Your First Domain
					</button>
				{/if}
			</div>
		{:else}
			<div class="space-y-2.5">
				{#each filteredSites as site (site.domain)}
					{@const siteUrl = buildSiteUrl(site, wb.networkStatus)}
					<div
						class="flex flex-wrap items-center justify-between gap-3 rounded-[16px] px-4 py-3.5 transition"
						style="background-color: var(--bg-surface); box-shadow: var(--shadow-card);"
					>
						<div class="flex min-w-0 flex-1 items-center gap-3">
							<div
								class="flex h-9 w-9 shrink-0 items-center justify-center rounded-[10px]"
								style="background-color: var(--bg-elevated); color: {site.https
									? 'var(--accent)'
									: 'var(--text-secondary)'};"
							>
								{#if site.https}
									<ShieldCheck class="h-4 w-4" />
								{:else}
									<Globe class="h-4 w-4" />
								{/if}
							</div>

							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-center gap-2">
									<button
										type="button"
										onclick={() => api.openExternalUrl(siteUrl)}
										class="font-mono text-sm font-semibold hover:underline"
										style="color: var(--text-primary);"
									>
										{siteUrl}
									</button>
									<span
										class="wb-pill font-mono text-[10px] uppercase"
										style={site.server === 'caddy'
											? 'background-color: var(--accent-soft); color: var(--accent);'
											: ''}
									>
										{site.server}
									</span>
									<span class="wb-pill font-mono text-[10px] uppercase">
										{site.mode === 'proxy'
											? `Proxy :${site.proxy_port ?? 5173}`
											: site.mode}
									</span>
									{#if site.https}
										<span
											class="wb-pill font-mono text-[10px]"
											style="color: var(--success);"
										>
											HTTPS
										</span>
									{/if}
									{#if site.expose_lan}
										<span class="wb-pill font-mono text-[10px]">LAN</span>
									{/if}
								</div>
								<div class="mt-0.5 truncate font-mono text-[11px]" style="color: var(--text-muted);">
									{site.project_path}{site.docroot_subpath ? `/${site.docroot_subpath}` : ''}
								</div>
							</div>
						</div>

						<div class="flex items-center gap-1.5">
							<button
								type="button"
								onclick={() => copyUrl(site.domain, siteUrl)}
								class="wb-btn px-2.5"
								title="Copy URL"
							>
								{#if copiedDomain === site.domain}
									<Check class="h-3.5 w-3.5" style="color: var(--success);" />
								{:else}
									<Copy class="h-3.5 w-3.5" />
								{/if}
							</button>
							<button
								type="button"
								onclick={() => api.openExternalUrl(siteUrl)}
								class="wb-btn px-2.5"
								title="Open in Browser"
							>
								<ExternalLink class="h-3.5 w-3.5" />
							</button>
							<button
								type="button"
								onclick={() => openEditForm(site)}
								class="wb-btn px-2.5"
								title="Edit Domain"
							>
								<Pencil class="h-3.5 w-3.5" />
							</button>
							<button
								type="button"
								onclick={() => goto(`/projects/detail?path=${encodeURIComponent(site.project_path)}`)}
								class="wb-btn px-2.5"
								title="Open Project"
							>
								<FolderOpen class="h-3.5 w-3.5" />
							</button>
							<button
								type="button"
								onclick={() => wb.removeLocalSite(site.domain)}
								class="wb-btn px-2.5 hover:text-[var(--danger)]"
								title="Unlink Domain"
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
