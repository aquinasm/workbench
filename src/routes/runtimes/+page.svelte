<script lang="ts">
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { formatBytes } from '$lib/format';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import TechIcon from '$lib/components/TechIcon.svelte';
	import type { PhpIniSettings, PhpVersionDetails } from '$lib/types';
	import {
		Download,
		Trash2,
		Star,
		RefreshCw,
		Terminal,
		CheckCircle2,
		HardDrive,
		SlidersHorizontal,
		FolderOpen
	} from 'lucide-svelte';

	type TabKey = 'php' | 'node' | 'composer' | 'flutter' | 'rust';
	let activeTab = $state<TabKey>('php');

	const tabs: { key: TabKey; label: string }[] = [
		{ key: 'php', label: 'PHP' },
		{ key: 'node', label: 'Node.js' },
		{ key: 'composer', label: 'Composer' },
		{ key: 'flutter', label: 'Flutter' },
		{ key: 'rust', label: 'Rust (rustup)' }
	];

	const filteredRuntimes = $derived(wb.runtimes.filter((r) => r.tool === activeTab));
	const totalRuntimeDisk = $derived(
		wb.runtimes.reduce((sum, r) => sum + (r.installed ? r.disk_bytes : 0), 0)
	);
	const installedCount = $derived(wb.runtimes.filter((r) => r.installed).length);

	let pendingUninstall = $state<{ tool: string; version: string; bytes: number } | null>(null);
	let confirmUninstallOpen = $state(false);

	let rustupBusy = $state(false);
	let customToolchain = $state('');

	// PHP Extensions & php.ini inspector state
	let phpInspectVersion = $state<string | null>(null);
	let phpDetails = $state<PhpVersionDetails | null>(null);
	let phpIniDraft = $state<PhpIniSettings | null>(null);
	let phpLoading = $state(false);
	let phpExtQuery = $state('');

	let phpReqSeq = 0;

	async function togglePhpInspector(version: string) {
		if (phpInspectVersion === version) {
			phpReqSeq++;
			phpInspectVersion = null;
			phpDetails = null;
			phpIniDraft = null;
			return;
		}
		const seq = ++phpReqSeq;
		phpInspectVersion = version;
		phpLoading = true;
		try {
			const details = await api.inspectPhpVersion(version);
			if (seq !== phpReqSeq) return;
			phpDetails = details;
			phpIniDraft = { ...details.settings };
		} catch (e) {
			if (seq !== phpReqSeq) return;
			wb.showToast(`Failed inspecting PHP ${version}: ${e}`, 'error');
			phpInspectVersion = null;
		} finally {
			if (seq === phpReqSeq) {
				phpLoading = false;
			}
		}
	}

	async function handleToggleExt(version: string, extName: string, enable: boolean) {
		try {
			const details = await api.togglePhpExtension(version, extName, enable);
			phpDetails = details;
			wb.showToast(`${enable ? 'Enabled' : 'Disabled'} PHP extension: ${extName}`, 'success');
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		}
	}

	async function handleSavePhpIni(version: string) {
		if (!phpIniDraft) return;
		try {
			const details = await api.updatePhpIniSettings(version, phpIniDraft);
			phpDetails = details;
			phpIniDraft = { ...details.settings };
			wb.showToast(`Saved php.ini for PHP ${version}`, 'success');
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		}
	}

	function askUninstall(tool: string, version: string, bytes: number) {
		pendingUninstall = { tool, version, bytes };
		confirmUninstallOpen = true;
	}

	async function doUninstall() {
		if (!pendingUninstall) return;
		const { tool, version } = pendingUninstall;
		pendingUninstall = null;
		await wb.uninstallRuntime(tool, version);
	}

	async function installRustToolchain(name: string) {
		const trimmed = name.trim();
		if (!trimmed) return;
		rustupBusy = true;
		try {
			await api.installRustupToolchain(trimmed);
			wb.showToast(`Installed Rust toolchain: ${trimmed}`, 'success');
			customToolchain = '';
			await wb.refreshRustup();
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			rustupBusy = false;
		}
	}

	async function setRustDefault(name: string) {
		rustupBusy = true;
		try {
			await api.setDefaultRustupToolchain(name);
			wb.showToast(`Default Rust toolchain set to ${name}`, 'success');
			await wb.refreshRustup();
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			rustupBusy = false;
		}
	}

	async function uninstallRustToolchain(name: string) {
		rustupBusy = true;
		try {
			await api.uninstallRustupToolchain(name);
			wb.showToast(`Uninstalled Rust toolchain: ${name}`, 'info');
			await wb.refreshRustup();
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			rustupBusy = false;
		}
	}
</script>

<div class="mx-auto max-w-6xl space-y-5">
	<header class="flex flex-wrap items-end justify-between gap-3 border-b pb-4" style="border-color: var(--border-subtle);">
		<div>
			<h1 class="text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
				Runtimes
			</h1>
			<p class="mt-0.5 font-mono text-[11px]" style="color: var(--text-secondary);">
				{installedCount} installed · <strong>{formatBytes(totalRuntimeDisk)}</strong> managed on disk
			</p>
		</div>

		<div class="flex flex-wrap items-center gap-2">
			<button type="button" onclick={() => wb.syncShims()} class="wb-btn">
				<Terminal class="h-3.5 w-3.5" />
				Sync Shims
			</button>
			<button
				type="button"
				disabled={wb.loadingRuntimes}
				onclick={() => {
					wb.refreshRuntimes();
					wb.refreshRustup();
				}}
				class="wb-btn"
			>
				<RefreshCw class="h-3.5 w-3.5 {wb.loadingRuntimes ? 'animate-spin' : ''}" />
				Refresh
			</button>
		</div>
	</header>

	{#if wb.shellStatus && !wb.shellStatus.installed}
		<div
			class="flex flex-wrap items-center justify-between gap-3 rounded-xl border px-4 py-3 text-xs"
			style="background-color: var(--accent-soft); border-color: var(--border-subtle); color: var(--text-primary);"
		>
			<span>
				<strong>Shell integration is not enabled yet.</strong> Enable it so
				<code class="font-mono">php</code>, <code class="font-mono">node</code>,
				<code class="font-mono">composer</code>, and <code class="font-mono">flutter</code>
				automatically switch versions in your terminal.
			</span>
			<a
				href="/settings"
				class="rounded-md px-3 py-1 font-semibold text-white transition hover:opacity-90"
				style="background-color: var(--accent);"
			>
				Enable in Settings
			</a>
		</div>
	{/if}

	<!-- Global Default Versions Bar -->
	<section
		class="rounded-xl border p-4 space-y-3"
		style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
	>
		<div class="flex items-center justify-between">
			<div>
				<h2 class="text-xs font-semibold uppercase tracking-wide" style="color: var(--text-secondary);">
					Global Default Versions
				</h2>
				<p class="text-[11px]" style="color: var(--text-muted);">
					Used whenever a project doesn't specify a pinned version in <code class="font-mono">.workbench.toml</code>.
				</p>
			</div>
		</div>

		<div class="grid grid-cols-2 gap-2.5 sm:grid-cols-5">
			{#each ['php', 'node', 'composer', 'flutter'] as tool}
				{@const installed = wb.runtimes.filter((r) => r.tool === tool && r.installed)}
				{@const currentDefault = installed.find((r) => r.is_default)?.version ?? ''}
				<label class="space-y-1">
					<span class="block text-[11px] font-medium uppercase" style="color: var(--text-secondary);">
						{tool}
					</span>
					<select
						disabled={installed.length === 0}
						value={currentDefault}
						onchange={(e) => {
							const v = (e.currentTarget as HTMLSelectElement).value;
							if (v) wb.setDefaultRuntime(tool, v);
						}}
						class="w-full rounded-lg border px-2 py-1.5 font-mono text-xs disabled:opacity-45"
						style="background-color: var(--bg-app); border-color: var(--border-subtle); color: var(--text-primary);"
					>
						{#if installed.length === 0}
							<option value="">Not installed</option>
						{:else}
							{#each installed as item}
								<option value={item.version}>
									{item.is_external
										? `${item.version.replace(/^ext-/, '')} (External · ${item.source ?? 'System'})`
										: `v${item.version}`}
								</option>
							{/each}
						{/if}
					</select>
				</label>
			{/each}

			<label class="space-y-1">
				<span class="block text-[11px] font-medium uppercase" style="color: var(--text-secondary);">
					rust
				</span>
				<select
					disabled={!wb.rustup.available || wb.rustup.toolchains.length === 0 || rustupBusy}
					value={wb.rustup.toolchains.find((t) => t.is_default)?.name ?? ''}
					onchange={(e) => {
						const v = (e.currentTarget as HTMLSelectElement).value;
						if (v) setRustDefault(v);
					}}
					class="w-full rounded-lg border px-2 py-1.5 font-mono text-xs disabled:opacity-45"
					style="background-color: var(--bg-app); border-color: var(--border-subtle); color: var(--text-primary);"
				>
					{#if !wb.rustup.available || wb.rustup.toolchains.length === 0}
						<option value="">No rustup</option>
					{:else}
						{#each wb.rustup.toolchains as tc}
							<option value={tc.name}>{tc.name}</option>
						{/each}
					{/if}
				</select>
			</label>
		</div>
	</section>

	<!-- Segmented Tabs -->
	<div
		class="inline-flex flex-wrap gap-1 rounded-xl border p-1"
		style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
	>
		{#each tabs as tab}
			{@const isActive = activeTab === tab.key}
			{@const count =
				tab.key === 'rust'
					? wb.rustup.toolchains.length
					: wb.runtimes.filter((r) => r.tool === tab.key && r.installed).length}
			<button
				type="button"
				onclick={() => (activeTab = tab.key)}
				class="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium transition"
				style="
					background-color: {isActive ? 'var(--accent-soft)' : 'transparent'};
					color: {isActive ? 'var(--accent)' : 'var(--text-secondary)'};
				"
			>
				<TechIcon name={tab.key} class="h-3.5 w-3.5" />
				<span>{tab.label}</span>
				{#if count > 0}
					<span
						class="rounded-full px-1.5 py-0.5 text-[10px] font-semibold"
						style="background-color: var(--bg-app); color: var(--text-primary);"
					>
						{count}
					</span>
				{/if}
			</button>
		{/each}
	</div>

	{#if activeTab === 'rust'}
		<section
			class="rounded-xl border p-5 space-y-4"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div>
					<h2 class="text-sm font-semibold" style="color: var(--text-primary);">
						Rust Toolchains (via <code class="font-mono">rustup</code>)
					</h2>
					<p class="text-xs" style="color: var(--text-secondary);">
						Workbench delegates Rust version management directly to <code class="font-mono">rustup</code>
						to avoid duplicating gigabytes of toolchains.
					</p>
				</div>

				{#if wb.rustup.available}
					<div class="flex items-center gap-1.5">
						<button
							type="button"
							disabled={rustupBusy}
							onclick={() => installRustToolchain('stable')}
							class="rounded-lg border px-2.5 py-1.5 text-xs font-medium transition hover:opacity-85 disabled:opacity-40"
							style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
						>
							+ stable
						</button>
						<button
							type="button"
							disabled={rustupBusy}
							onclick={() => installRustToolchain('nightly')}
							class="rounded-lg border px-2.5 py-1.5 text-xs font-medium transition hover:opacity-85 disabled:opacity-40"
							style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
						>
							+ nightly
						</button>
					</div>
				{/if}
			</div>

			{#if !wb.rustup.available}
				<div class="rounded-lg border p-4 text-xs" style="border-color: var(--border-subtle); color: var(--text-secondary);">
					<code class="font-mono">rustup</code> was not detected on your PATH. Install it from
					<code class="font-mono">https://rustup.rs</code> to manage multiple Rust toolchains here.
				</div>
			{:else}
				<div class="flex items-center gap-2">
					<input
						type="text"
						bind:value={customToolchain}
						placeholder="Install specific version (e.g. 1.85.0 or beta)..."
						class="flex-1 rounded-lg border px-3 py-1.5 text-xs outline-none"
						style="background-color: var(--bg-app); border-color: var(--border-subtle); color: var(--text-primary);"
					/>
					<button
						type="button"
						disabled={rustupBusy || !customToolchain.trim()}
						onclick={() => installRustToolchain(customToolchain)}
						class="rounded-lg px-3.5 py-1.5 text-xs font-semibold text-white transition hover:opacity-90 disabled:opacity-40"
						style="background-color: var(--accent);"
					>
						{rustupBusy ? 'Working...' : 'Install Toolchain'}
					</button>
				</div>

				<div class="divide-y rounded-lg border" style="border-color: var(--border-subtle);">
					{#each wb.rustup.toolchains as tc (tc.name)}
						<div class="flex items-center justify-between gap-3 px-4 py-3">
							<div class="flex items-center gap-2">
								<span class="font-mono text-xs font-semibold" style="color: var(--text-primary);">
									{tc.name}
								</span>
								{#if tc.is_default}
									<span
										class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold"
										style="background-color: var(--accent-soft); color: var(--accent);"
									>
										<Star class="h-2.5 w-2.5" />
										Default
									</span>
								{/if}
							</div>
							<div class="flex items-center gap-2">
								{#if !tc.is_default}
									<button
										type="button"
										disabled={rustupBusy}
										onclick={() => setRustDefault(tc.name)}
										class="rounded-md border px-2.5 py-1 text-xs font-medium transition hover:opacity-85 disabled:opacity-40"
										style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
									>
										Set Default
									</button>
									<button
										type="button"
										disabled={rustupBusy}
										onclick={() => uninstallRustToolchain(tc.name)}
										class="rounded-md p-1.5 transition hover:opacity-75 disabled:opacity-40"
										style="color: var(--danger);"
										title="Uninstall toolchain"
									>
										<Trash2 class="h-3.5 w-3.5" />
									</button>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</section>
	{:else}
		<div
			class="divide-y overflow-hidden rounded-xl border"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			{#each filteredRuntimes as rt (`${rt.tool}:${rt.version}`)}
				{@const progKey = `${rt.tool}:${rt.version}`}
				{@const progress = wb.downloadProgress[progKey]}
				<div class="px-4 py-3.5 space-y-2">
					<div class="flex items-center justify-between gap-4">
						<div class="min-w-0 flex-1">
							<div class="flex flex-wrap items-center gap-2">
								<span class="text-sm font-semibold" style="color: var(--text-primary);">
									{rt.label}
								</span>
								<span
									class="border px-1.5 py-0.5 font-mono text-[11px]"
									style="border-color: var(--border-subtle); background-color: var(--bg-elevated); color: var(--text-secondary);"
								>
									{rt.is_external ? rt.version.replace(/^ext-/, 'v') : `v${rt.version}`}
								</span>
								{#if rt.is_external}
									<span
										class="border px-1.5 py-0.5 font-mono text-[11px] font-semibold uppercase"
										style="border-color: var(--border-strong); background-color: var(--bg-elevated); color: var(--text-secondary);"
									>
										EXTERNAL{rt.source ? ` · ${rt.source}` : ''}
									</span>
								{/if}
								{#if rt.is_default}
									<span
										class="inline-flex items-center gap-1 px-2 py-0.5 text-[11px] font-semibold"
										style="background-color: var(--accent-soft); color: var(--accent);"
									>
										<Star class="h-3 w-3" />
										Global Default
									</span>
								{:else if rt.installed && !rt.is_external}
									<span
										class="inline-flex items-center gap-1 px-2 py-0.5 text-[11px] font-medium"
										style="background-color: var(--bg-elevated); color: var(--success);"
									>
										<CheckCircle2 class="h-3 w-3" />
										Managed
									</span>
								{/if}
							</div>

							<div class="mt-0.5 flex items-center gap-3 text-[11px]" style="color: var(--text-muted);">
								{#if rt.is_external}
									<span>External binary</span>
									{#if rt.install_path}
										<span class="selectable truncate font-mono">{rt.install_path}</span>
									{/if}
								{:else if rt.installed}
									<span class="inline-flex items-center gap-1 tabular-nums">
										<HardDrive class="h-3 w-3" />
										{formatBytes(rt.disk_bytes)} on disk
									</span>
									{#if rt.install_path}
										<span class="selectable truncate font-mono">{rt.install_path}</span>
									{/if}
								{:else}
									<span>Download ~{rt.size_hint_mb} MB · Channel: {rt.channel}</span>
								{/if}
							</div>
						</div>

						<div class="flex shrink-0 items-center gap-2">
							{#if progress}
								<span class="text-xs font-medium tabular-nums" style="color: var(--accent);">
									{progress.stage === 'extracting'
										? 'Extracting...'
										: `${progress.percent}%`}
								</span>
							{:else if rt.installed}
								{#if rt.tool === 'php' && !rt.is_external}
									<button
										type="button"
										onclick={() => togglePhpInspector(rt.version)}
										class="inline-flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs font-medium transition hover:opacity-85"
										style="border-color: var(--border-subtle); background-color: {phpInspectVersion === rt.version ? 'var(--accent-soft)' : 'var(--bg-app)'}; color: {phpInspectVersion === rt.version ? 'var(--accent)' : 'var(--text-primary)'};"
									>
										<SlidersHorizontal class="h-3 w-3" />
										Extensions & php.ini
									</button>
								{/if}
								{#if !rt.is_default}
									<button
										type="button"
										onclick={() => wb.setDefaultRuntime(rt.tool, rt.version)}
										class="rounded-md border px-2.5 py-1 text-xs font-medium transition hover:opacity-85"
										style="border-color: var(--border-subtle); background-color: var(--bg-app); color: var(--text-primary);"
									>
										Set Default
									</button>
								{/if}
								{#if !rt.is_external}
									<button
										type="button"
										onclick={() => askUninstall(rt.tool, rt.version, rt.disk_bytes)}
										class="inline-flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs font-medium transition hover:opacity-85"
										style="border-color: var(--border-subtle); color: var(--danger);"
									>
										<Trash2 class="h-3 w-3" />
										Uninstall
									</button>
								{/if}
							{:else}
								<button
									type="button"
									disabled={!rt.available_on_platform}
									onclick={() => wb.installRuntime(rt.tool, rt.version)}
									class="inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold text-white transition hover:opacity-90 disabled:opacity-40"
									style="background-color: var(--accent);"
								>
									<Download class="h-3.5 w-3.5" />
									{rt.available_on_platform ? 'Install' : 'Unsupported OS'}
								</button>
							{/if}
						</div>
					</div>

					{#if progress}
						<div class="h-1.5 w-full overflow-hidden rounded-full" style="background-color: var(--bg-app);">
							<div
								class="h-full rounded-full transition-all duration-150"
								style="width: {progress.percent}%; background-color: var(--accent);"
							></div>
						</div>
					{/if}

					{#if rt.tool === 'php' && phpInspectVersion === rt.version}
						<div
							class="mt-3 rounded-xl border p-4 space-y-4 text-xs"
							style="border-color: var(--border-subtle); background-color: var(--bg-app);"
						>
							{#if phpLoading || !phpDetails || !phpIniDraft}
								<div style="color: var(--text-muted);">Loading PHP extensions & php.ini...</div>
							{:else}
								<!-- php.ini Quick Tuning -->
								<div class="space-y-2.5">
									<div class="flex flex-wrap items-center justify-between gap-2">
										<div>
											<span class="font-semibold" style="color: var(--text-primary);">
												php.ini Configuration
											</span>
											<span class="selectable ml-2 font-mono text-[11px]" style="color: var(--text-muted);">
												{phpDetails.ini_path}
											</span>
										</div>
										<button
											type="button"
											onclick={() => api.revealInFileManager(phpDetails!.ini_path)}
											class="inline-flex items-center gap-1 rounded border px-2 py-1 text-[11px] font-medium hover:opacity-80"
											style="border-color: var(--border-subtle); background-color: var(--bg-surface); color: var(--text-primary);"
										>
											<FolderOpen class="h-3 w-3" />
											Reveal php.ini
										</button>
									</div>

									<div class="grid grid-cols-2 gap-2.5 sm:grid-cols-5">
										<label class="space-y-1">
											<span class="block text-[11px]" style="color: var(--text-secondary);">memory_limit</span>
											<input
												type="text"
												bind:value={phpIniDraft.memory_limit}
												class="w-full rounded border px-2 py-1 font-mono text-xs"
												style="border-color: var(--border-subtle); background-color: var(--bg-surface); color: var(--text-primary);"
											/>
										</label>
										<label class="space-y-1">
											<span class="block text-[11px]" style="color: var(--text-secondary);">upload_max_filesize</span>
											<input
												type="text"
												bind:value={phpIniDraft.upload_max_filesize}
												class="w-full rounded border px-2 py-1 font-mono text-xs"
												style="border-color: var(--border-subtle); background-color: var(--bg-surface); color: var(--text-primary);"
											/>
										</label>
										<label class="space-y-1">
											<span class="block text-[11px]" style="color: var(--text-secondary);">post_max_size</span>
											<input
												type="text"
												bind:value={phpIniDraft.post_max_size}
												class="w-full rounded border px-2 py-1 font-mono text-xs"
												style="border-color: var(--border-subtle); background-color: var(--bg-surface); color: var(--text-primary);"
											/>
										</label>
										<label class="space-y-1">
											<span class="block text-[11px]" style="color: var(--text-secondary);">max_execution_time</span>
											<input
												type="text"
												bind:value={phpIniDraft.max_execution_time}
												class="w-full rounded border px-2 py-1 font-mono text-xs"
												style="border-color: var(--border-subtle); background-color: var(--bg-surface); color: var(--text-primary);"
											/>
										</label>
										<div class="flex flex-col justify-end">
											<button
												type="button"
												onclick={() => handleSavePhpIni(rt.version)}
												class="rounded-md px-3 py-1.5 text-xs font-semibold text-white transition hover:opacity-90"
												style="background-color: var(--accent);"
											>
												Save php.ini
											</button>
										</div>
									</div>

									<label class="inline-flex items-center gap-2 text-xs" style="color: var(--text-secondary);">
										<input type="checkbox" bind:checked={phpIniDraft.opcache_enable} />
										<span>Enable OPcache (<code class="font-mono">opcache.enable = 1</code>)</span>
									</label>
								</div>

								<!-- Extensions Grid -->
								<div class="border-t pt-3 space-y-2.5" style="border-color: var(--border-subtle);">
									<div class="flex flex-wrap items-center justify-between gap-2">
										<div class="font-semibold" style="color: var(--text-primary);">
											PHP Extensions ({phpDetails.extensions.filter((e) => e.loaded).length} loaded)
										</div>
										<input
											type="search"
											bind:value={phpExtQuery}
											placeholder="Filter extensions (e.g. pdo, redis, gd)..."
											class="w-56 rounded border px-2.5 py-1 text-xs outline-none"
											style="border-color: var(--border-subtle); background-color: var(--bg-surface); color: var(--text-primary);"
										/>
									</div>

									<div class="grid max-h-56 grid-cols-2 gap-1.5 overflow-y-auto sm:grid-cols-3">
										{#each phpDetails.extensions.filter(
											(e) =>
												!phpExtQuery.trim() ||
												e.name.toLowerCase().includes(phpExtQuery.trim().toLowerCase())
										) as ext (ext.name)}
											<div
												class="flex items-center justify-between rounded-lg border px-2.5 py-1.5"
												style="border-color: var(--border-subtle); background-color: var(--bg-surface);"
											>
												<div class="min-w-0">
													<div class="flex items-center gap-1.5">
														<span class="truncate font-mono text-[11.5px] font-medium" style="color: var(--text-primary);">
															{ext.name}
														</span>
														{#if ext.is_laravel_core}
															<span class="rounded bg-red-500/12 px-1 py-0.2 text-[9px] font-semibold text-red-600 dark:text-red-400">
																Laravel
															</span>
														{/if}
													</div>
												</div>

												{#if ext.built_in}
													<span class="text-[10px] font-medium" style="color: var(--success);">
														Built-in
													</span>
												{:else if ext.available_dynamic || ext.enabled_in_ini}
													<input
														type="checkbox"
														checked={ext.loaded || ext.enabled_in_ini}
														onchange={(e) =>
															handleToggleExt(
																rt.version,
																ext.name,
																(e.currentTarget as HTMLInputElement).checked
															)}
														aria-label="Toggle {ext.name}"
													/>
												{:else}
													<span class="text-[10px]" style="color: var(--text-muted);">
														Missing
													</span>
												{/if}
											</div>
										{/each}
									</div>
								</div>
							{/if}
						</div>
					{/if}
				</div>
			{/each}
		</div>
	{/if}
</div>

<ConfirmDialog
	bind:open={confirmUninstallOpen}
	title="Uninstall {pendingUninstall?.tool} {pendingUninstall?.version}?"
	confirmLabel="Uninstall & Free {formatBytes(pendingUninstall?.bytes ?? 0)}"
	destructive={true}
	onConfirm={doUninstall}
	onCancel={() => (pendingUninstall = null)}
>
	<p>
		This removes the managed runtime binaries from <code class="font-mono">~/.workbench/runtimes</code>
		and immediately frees <strong>{formatBytes(pendingUninstall?.bytes ?? 0)}</strong>.
	</p>
</ConfirmDialog>
