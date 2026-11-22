<script lang="ts">
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import type { DeleteMode, UninstallReport } from '$lib/types';
	import {
		FolderPlus,
		FolderOpen,
		Trash2,
		Terminal,
		ShieldCheck,
		AlertTriangle,
		CheckCircle2,
		Globe,
		Wifi,
		Sun,
		Moon,
		Monitor,
		SlidersHorizontal
	} from 'lucide-svelte';

	let uninstallConfirmOpen = $state(false);
	let uninstallReport = $state<UninstallReport | null>(null);
	let shellBusy = $state(false);

	const themes = [
		{ id: 'system', label: 'System', icon: Monitor },
		{ id: 'light', label: 'Light', icon: Sun },
		{ id: 'dark', label: 'Dark', icon: Moon }
	] as const;

	const deleteModes: { id: DeleteMode; label: string; desc: string }[] = [
		{ id: 'permanent', label: 'Permanent', desc: 'Instant disk space recovery' },
		{ id: 'trash', label: 'OS Trash', desc: 'Reversible via Trash / Recycle Bin' }
	];

	async function installShell() {
		shellBusy = true;
		try {
			wb.shellStatus = await api.installShellIntegration();
			wb.showToast('Shell integration installed cleanly', 'success');
		} catch (e) {
			wb.showToast(`Shell integration failed: ${e}`, 'error');
		} finally {
			shellBusy = false;
		}
	}

	async function removeShell() {
		shellBusy = true;
		try {
			await api.removeShellIntegration();
			await wb.refreshShellStatus();
			wb.showToast('Shell integration removed cleanly', 'info');
		} catch (e) {
			wb.showToast(`Failed removing shell integration: ${e}`, 'error');
		} finally {
			shellBusy = false;
		}
	}

	async function performCleanUninstall() {
		try {
			const report = await api.cleanUninstall();
			uninstallReport = report;
			await wb.refreshShellStatus();
			wb.showToast('Workbench data and shell hooks cleaned safely', 'success');
		} catch (e) {
			wb.showToast(`Clean uninstall failed: ${e}`, 'error');
		}
	}
</script>

<div class="mx-auto max-w-6xl space-y-6">
	<header
		class="flex flex-wrap items-end justify-between gap-4 border-b pb-4"
		style="border-color: var(--border-subtle);"
	>
		<div>
			<h1 class="text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
				Settings
			</h1>
			<p class="mt-0.5 text-xs" style="color: var(--text-muted);">
				Configure appearance, workspaces, PATH shims, local DNS/HTTPS, and storage preferences.
			</p>
		</div>
	</header>

	<div class="settings-grid">
		<!-- Appearance & Storage Preferences -->
		<section class="rounded-2xl p-5 space-y-5" style="background-color: var(--bg-surface);">
			<div class="flex items-center gap-3">
				<div
					class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl"
					style="background-color: var(--accent-soft); color: var(--accent);"
				>
					<SlidersHorizontal class="h-4 w-4" />
				</div>
				<div>
					<h2 class="text-sm font-semibold" style="color: var(--text-primary);">
						Appearance & Cleanup
					</h2>
					<p class="text-xs" style="color: var(--text-muted);">
						Theme preference and artifact cleanup behavior.
					</p>
				</div>
			</div>

			<div class="space-y-4">
				<div class="space-y-2">
					<span class="block wb-kicker">Theme</span>
					<div class="grid grid-cols-3 gap-2">
						{#each themes as t}
							{@const Icon = t.icon}
							{@const active = wb.config.theme === t.id}
							<button
								type="button"
								onclick={() => wb.saveConfig({ theme: t.id })}
								class="flex items-center justify-center gap-2 rounded-xl py-2.5 text-xs font-medium transition"
								style="
									background-color: {active ? 'var(--accent-soft)' : 'var(--bg-app)'};
									color: {active ? 'var(--accent)' : 'var(--text-secondary)'};
								"
							>
								<Icon class="h-3.5 w-3.5" />
								{t.label}
							</button>
						{/each}
					</div>
				</div>

				<div class="space-y-2">
					<span class="block wb-kicker">Deletion Mode</span>
					<div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
						{#each deleteModes as dm}
							{@const active = wb.config.delete_mode === dm.id}
							<button
								type="button"
								onclick={() => wb.saveConfig({ delete_mode: dm.id })}
								class="rounded-xl p-3 text-left transition"
								style="
									background-color: {active ? 'var(--accent-soft)' : 'var(--bg-app)'};
								"
							>
								<div
									class="text-xs font-semibold"
									style="color: {active ? 'var(--accent)' : 'var(--text-primary)'};"
								>
									{dm.label}
								</div>
								<div class="mt-0.5 text-[11px]" style="color: var(--text-muted);">
									{dm.desc}
								</div>
							</button>
						{/each}
					</div>
				</div>

				<div class="flex items-center justify-between gap-4 rounded-xl p-3.5" style="background-color: var(--bg-app);">
					<div>
						<div class="text-xs font-semibold" style="color: var(--text-primary);">
							Stale Project Threshold
						</div>
						<div class="text-[11px]" style="color: var(--text-muted);">
							Days of inactivity before suggesting hibernation.
						</div>
					</div>
					<div class="flex items-center gap-2">
						<input
							type="number"
							min="1"
							max="365"
							value={wb.config.stale_days}
							onchange={(e) =>
								wb.saveConfig({
									stale_days: Math.max(1, Number((e.currentTarget as HTMLInputElement).value) || 14)
								})}
							class="w-20 px-3 py-1.5 text-center font-mono text-xs"
							style="color: var(--text-primary);"
						/>
						<span class="font-mono text-xs" style="color: var(--text-muted);">days</span>
					</div>
				</div>
			</div>
		</section>

		<!-- Workspace Scan Roots -->
		<section class="flex flex-col justify-between rounded-2xl p-5 space-y-4" style="background-color: var(--bg-surface);">
			<div class="space-y-4">
				<div class="flex items-start justify-between gap-3">
					<div class="flex items-center gap-3">
						<div
							class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl"
							style="background-color: var(--accent-soft); color: var(--accent);"
						>
							<FolderPlus class="h-4 w-4" />
						</div>
						<div>
							<h2 class="text-sm font-semibold" style="color: var(--text-primary);">
								Workspace Folders
							</h2>
							<p class="text-xs" style="color: var(--text-muted);">
								Directories scanned for Laravel, SvelteKit, Flutter, and Rust projects.
							</p>
						</div>
					</div>
					<button type="button" onclick={() => wb.addWorkspaceFolder()} class="wb-btn-primary shrink-0">
						<FolderPlus class="h-3.5 w-3.5" />
						Add Folder
					</button>
				</div>

				{#if wb.config.scan_roots.length === 0}
					<div
						class="rounded-xl p-6 text-center font-mono text-xs"
						style="background-color: var(--bg-app); color: var(--text-muted);"
					>
						No workspace folders configured yet.
					</div>
				{:else}
					<div class="space-y-2">
						{#each wb.config.scan_roots as root (root)}
							<div
								class="flex items-center justify-between gap-2 rounded-xl px-3.5 py-2.5"
								style="background-color: var(--bg-app);"
							>
								<span class="selectable truncate font-mono text-xs" style="color: var(--text-primary);">
									{root}
								</span>
								<div class="flex shrink-0 items-center gap-1">
									<button
										type="button"
										title="Reveal in Finder / Explorer"
										onclick={() => api.revealInFileManager(root)}
										class="rounded-full p-1.5 transition hover:bg-[var(--bg-surface)]"
										style="color: var(--text-muted);"
									>
										<FolderOpen class="h-3.5 w-3.5" />
									</button>
									<button
										type="button"
										title="Remove scan root"
										onclick={() => wb.removeWorkspaceFolder(root)}
										class="rounded-full p-1.5 transition hover:bg-[var(--danger-soft)]"
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

			<p class="font-mono text-[11px]" style="color: var(--text-muted);">
				{wb.config.scan_roots.length} {wb.config.scan_roots.length === 1 ? 'folder' : 'folders'} · {wb.projects.length} discovered projects
			</p>
		</section>

		<!-- Shell Integration -->
		<section class="flex flex-col justify-between rounded-2xl p-5 space-y-4" style="background-color: var(--bg-surface);">
			<div class="space-y-4">
				<div class="flex items-start justify-between gap-3">
					<div class="flex items-start gap-3">
						<div
							class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl"
							style="background-color: var(--accent-soft); color: var(--accent);"
						>
							<Terminal class="h-4 w-4" />
						</div>
						<div>
							<h2 class="text-sm font-semibold" style="color: var(--text-primary);">
								Shell Integration (PATH Shims)
							</h2>
							<p class="mt-0.5 text-xs" style="color: var(--text-muted);">
								Auto-resolves pinned <code class="font-mono">php</code>, <code class="font-mono">node</code>,
								<code class="font-mono">composer</code>, and <code class="font-mono">flutter</code> versions per project.
							</p>
						</div>
					</div>

					{#if wb.shellStatus?.installed}
						<span
							class="inline-flex shrink-0 items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-medium"
							style="background-color: color-mix(in srgb, var(--success) 16%, transparent); color: var(--success);"
						>
							<CheckCircle2 class="h-3.5 w-3.5" />
							Installed
						</span>
					{/if}
				</div>

				{#if wb.shellStatus}
					<div
						class="selectable rounded-xl p-3.5 font-mono text-[11px] space-y-1.5"
						style="background-color: var(--bg-app); color: var(--text-secondary);"
					>
						<div class="flex justify-between gap-2">
							<span style="color: var(--text-muted);">Shell</span>
							<strong style="color: var(--text-primary);">{wb.shellStatus.shell_name}</strong>
						</div>
						<div class="flex justify-between gap-2">
							<span class="shrink-0" style="color: var(--text-muted);">Profile</span>
							<span class="truncate">{wb.shellStatus.profile_path}</span>
						</div>
						<div class="flex justify-between gap-2">
							<span class="shrink-0" style="color: var(--text-muted);">Shims</span>
							<span class="truncate">{wb.shellStatus.shims_path}</span>
						</div>
					</div>
				{/if}
			</div>

			<div class="flex flex-wrap items-center gap-2">
				<button type="button" disabled={shellBusy} onclick={installShell} class="wb-btn-primary">
					{wb.shellStatus?.installed ? 'Reinstall Shims' : 'Install Shell Integration'}
				</button>
				{#if wb.shellStatus?.installed}
					<button type="button" disabled={shellBusy} onclick={removeShell} class="wb-btn">
						Remove
					</button>
				{/if}
			</div>
		</section>

		<!-- Local DNS & SSL Certificates -->
		<section class="flex flex-col justify-between rounded-2xl p-5 space-y-4" style="background-color: var(--bg-surface);">
			<div class="space-y-4">
				<div class="flex items-start justify-between gap-3">
					<div class="flex items-start gap-3">
						<div
							class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl"
							style="background-color: var(--accent-soft); color: var(--accent);"
						>
							<Globe class="h-4 w-4" />
						</div>
						<div>
							<h2 class="text-sm font-semibold" style="color: var(--text-primary);">
								Local DNS & HTTPS
							</h2>
							<p class="mt-0.5 text-xs" style="color: var(--text-muted);">
								Maps linked <code class="font-mono">.test</code> domains and trusts Caddy's local root CA.
							</p>
						</div>
					</div>

					{#if wb.networkStatus}
						{@const allSynced = wb.networkStatus.missing_domains.length === 0}
						<span
							class="inline-flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-medium"
							style="background-color: {allSynced
								? 'color-mix(in srgb, var(--success) 16%, transparent)'
								: 'color-mix(in srgb, var(--warning) 16%, transparent)'}; color: {allSynced
								? 'var(--success)'
								: 'var(--warning)'};"
						>
							<span
								class="h-1.5 w-1.5 rounded-full"
								style="background-color: {allSynced ? 'var(--success)' : 'var(--warning)'};"
							></span>
							{allSynced ? 'Synced' : `${wb.networkStatus.missing_domains.length} Pending`}
						</span>
					{/if}
				</div>

				{#if wb.networkStatus}
					<div
						class="selectable rounded-xl p-3.5 font-mono text-[11px] space-y-1.5"
						style="background-color: var(--bg-app); color: var(--text-secondary);"
					>
						<div class="flex items-center justify-between gap-2">
							<span style="color: var(--text-muted);">Hosts file</span>
							<strong class="truncate" style="color: var(--text-primary);">{wb.networkStatus.hosts_path}</strong>
						</div>
						<div class="flex items-center justify-between gap-2">
							<span class="inline-flex items-center gap-1.5" style="color: var(--text-muted);">
								<Wifi class="h-3 w-3" style="color: var(--accent);" />
								LAN IP
							</span>
							<strong style="color: var(--text-primary);">{wb.networkStatus.lan_ip ?? 'Not detected'}</strong>
						</div>
						<div class="flex items-center justify-between gap-2">
							<span style="color: var(--text-muted);">HTTPS Port</span>
							<strong style="color: var(--text-primary);">:{wb.networkStatus.caddy_https_port}</strong>
						</div>
						<div class="flex items-center justify-between gap-2">
							<span class="shrink-0" style="color: var(--text-muted);">Synced ({wb.networkStatus.synced_domains.length})</span>
							<span class="truncate" style="color: var(--text-primary);">
								{wb.networkStatus.synced_domains.length > 0 ? wb.networkStatus.synced_domains.join(', ') : 'None'}
							</span>
						</div>
						{#if wb.networkStatus.missing_domains.length > 0}
							<div class="flex items-center justify-between gap-2">
								<span class="shrink-0" style="color: var(--warning);">Pending ({wb.networkStatus.missing_domains.length})</span>
								<span class="truncate" style="color: var(--warning);">
									{wb.networkStatus.missing_domains.join(', ')}
								</span>
							</div>
						{/if}
					</div>
				{/if}
			</div>

			<div class="flex flex-wrap items-center gap-2">
				<button
					type="button"
					disabled={wb.syncingDns}
					onclick={() => wb.syncHostsDns()}
					class="wb-btn-primary"
				>
					<Globe class="h-3.5 w-3.5" />
					{wb.syncingDns ? 'Syncing DNS...' : 'Sync .test DNS'}
				</button>
				<button
					type="button"
					disabled={wb.trustingCa || !wb.networkStatus?.caddy_available}
					onclick={() => wb.trustLocalCa()}
					class="wb-btn"
				>
					<ShieldCheck class="h-3.5 w-3.5" />
					{wb.trustingCa ? 'Trusting CA...' : 'Trust Local SSL CA'}
				</button>
			</div>
		</section>
	</div>

	<!-- Clean Uninstall -->
	<section
		class="rounded-2xl p-5 space-y-4"
		style="background-color: color-mix(in srgb, var(--danger) 6%, var(--bg-surface));"
	>
		<div class="flex flex-wrap items-start justify-between gap-4">
			<div class="flex items-start gap-3">
				<div
					class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl"
					style="background-color: var(--danger-soft); color: var(--danger);"
				>
					<AlertTriangle class="h-4 w-4" />
				</div>
				<div>
					<h2 class="text-sm font-semibold" style="color: var(--text-primary);">
						Clean Uninstall Workbench Data
					</h2>
					<p class="mt-0.5 text-xs" style="color: var(--text-secondary);">
						Removes shell profile hooks and deletes only
						<code class="font-mono">{wb.shellStatus?.workbench_home ?? '~/.workbench'}</code>
						(shims, config, metadata, managed runtimes, and logs).
					</p>
				</div>
			</div>

			<button
				type="button"
				onclick={() => (uninstallConfirmOpen = true)}
				class="wb-btn-danger shrink-0"
			>
				Clean Uninstall...
			</button>
		</div>

		<div
			class="flex items-center gap-2 rounded-xl px-3.5 py-2.5 text-xs"
			style="background-color: var(--bg-app); color: var(--success);"
		>
			<ShieldCheck class="h-4 w-4 shrink-0" />
			<span>
				<strong>Project Safety Guarantee:</strong> Uninstalling Workbench never modifies or deletes any of your project folders or source code.
			</span>
		</div>

		{#if uninstallReport}
			<div
				class="rounded-xl p-3.5 font-mono text-xs space-y-1"
				style="background-color: var(--bg-app); color: var(--text-primary);"
			>
				<div><strong>Clean Uninstall Complete</strong></div>
				<div>Shell profiles cleaned: {uninstallReport.shell_profiles_cleaned.length}</div>
				<div>Workbench home removed: {uninstallReport.workbench_home_removed ? 'Yes' : 'Already clean'}</div>
				<div>User projects touched: <strong>No (0 files modified)</strong></div>
				<div class="pt-1 text-[11px]" style="color: var(--text-muted);">
					You can now safely delete the Workbench app bundle from Applications / Programs.
				</div>
			</div>
		{/if}
	</section>
</div>

<ConfirmDialog
	bind:open={uninstallConfirmOpen}
	title="Clean uninstall Workbench data?"
	confirmLabel="Remove Workbench Data"
	destructive={true}
	onConfirm={performCleanUninstall}
	onCancel={() => {}}
>
	<p>This will:</p>
	<ul class="mt-1.5 list-disc pl-5 space-y-1 text-xs">
		<li>Remove the <code class="font-mono"># &gt;&gt;&gt; Workbench &gt;&gt;&gt;</code> block from your shell profiles.</li>
		<li>Delete <code class="font-mono">{wb.shellStatus?.workbench_home ?? '~/.workbench'}</code> (config, shims, logs, and Workbench-managed runtimes).</li>
	</ul>
	<p class="mt-2.5 text-xs font-semibold" style="color: var(--success);">
		Your project folders and source code will NOT be touched or deleted.
	</p>
</ConfirmDialog>

<style>
	.settings-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(420px, 100%), 1fr));
		gap: 1rem;
	}
</style>
