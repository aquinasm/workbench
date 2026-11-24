<script lang="ts">
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import {
		RefreshCw,
		ArrowUpRight,
		CheckCircle2,
		ShieldCheck,
		Github,
		Bug,
		Tag
	} from 'lucide-svelte';

	const REPO_URL = 'https://github.com/aquinasm/workbench';
	const RELEASES_URL = `${REPO_URL}/releases`;
	const ISSUES_URL = `${REPO_URL}/issues`;
</script>

<div class="mx-auto max-w-6xl space-y-6">
	<header
		class="flex flex-wrap items-end justify-between gap-4 border-b pb-4"
		style="border-color: var(--border-subtle);"
	>
		<div>
			<h1 class="text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
				About
			</h1>
		</div>
	</header>

	<!-- Brand Hero & Description -->
	<section class="rounded-2xl p-6 space-y-5" style="background-color: var(--bg-surface);">
		<div class="flex flex-wrap items-center justify-between gap-6">
			<div class="flex items-center gap-4">
				<svg
					xmlns="http://www.w3.org/2000/svg"
					viewBox="0 0 1024 1024"
					width="56"
					height="56"
					fill="none"
					class="shrink-0"
					aria-label="Workbench logo"
				>
					<rect x="48" y="48" width="928" height="928" rx="224" fill="var(--bg-app)" />
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

				<div class="space-y-1">
					<div class="flex flex-wrap items-center gap-2.5">
						<h2 class="text-xl font-bold tracking-[-0.02em]" style="color: var(--text-primary);">
							Workbench
						</h2>
						<span
							class="rounded-full px-2.5 py-0.5 font-mono text-xs font-semibold"
							style="background-color: var(--accent-soft); color: var(--accent);"
						>
							v{wb.updateInfo.current_version}
						</span>
					</div>
					<p class="text-sm" style="color: var(--text-secondary);">
						Cross-platform local development environment.
					</p>
				</div>
			</div>

			<div class="flex flex-wrap items-center gap-2">
				{#if wb.updatingApp}
					<button type="button" disabled class="wb-btn-primary">
						<RefreshCw class="h-3.5 w-3.5 animate-spin" />
						{wb.appUpdateProgress?.message ?? 'Updating...'}
					</button>
				{:else if wb.updateInfo.update_available && wb.updateInfo.latest_version}
					<button
						type="button"
						onclick={() => wb.installAppUpdate()}
						class="wb-btn-primary"
					>
						Install v{wb.updateInfo.latest_version}
						<ArrowUpRight class="h-3.5 w-3.5" />
					</button>
				{:else}
					<button
						type="button"
						disabled={wb.checkingUpdates}
						onclick={() => wb.checkForUpdates(true)}
						class="wb-btn-primary"
					>
						<RefreshCw class="h-3.5 w-3.5 {wb.checkingUpdates ? 'animate-spin' : ''}" />
						{wb.checkingUpdates ? 'Checking...' : 'Check for Updates'}
					</button>
				{/if}

				{#if wb.updateInfo.checked && !wb.updateInfo.update_available}
					<span
						class="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 font-mono text-xs font-medium"
						style="background-color: color-mix(in srgb, var(--success) 16%, transparent); color: var(--success);"
					>
						<CheckCircle2 class="h-3.5 w-3.5" />
						Up to date
					</span>
				{/if}
			</div>
		</div>

		<div class="max-w-3xl space-y-3 text-sm leading-relaxed" style="color: var(--text-secondary);">
			<p>
				Workbench is a fast, native desktop local development environment built for managing your
				projects, databases, local services, and language runtimes from one place. It handles
				per-project runtime shims (PHP, Node, Composer, Flutter, and Rust), local
				<code class="font-mono text-xs">.test</code> domains with automatic HTTPS via Caddy,
				MariaDB/MySQL database administration and backups, and workspace maintenance without getting
				in your way.
			</p>
			<p style="color: var(--text-muted);">
				Created by <strong style="color: var(--text-primary);">Aquinas Marydasan</strong> with
				Tauri, Rust, and SvelteKit. Released as free and open-source software under the MIT License.
			</p>
		</div>

		<!-- GitHub Links -->
		<div class="flex flex-wrap items-center gap-2 pt-1">
			<button
				type="button"
				onclick={() => api.openExternalUrl(REPO_URL)}
				class="wb-btn"
			>
				<Github class="h-3.5 w-3.5" style="color: var(--accent);" />
				GitHub Repository
				<ArrowUpRight class="h-3.5 w-3.5 opacity-60" />
			</button>

			<button
				type="button"
				onclick={() => api.openExternalUrl(RELEASES_URL)}
				class="wb-btn"
			>
				<Tag class="h-3.5 w-3.5" style="color: var(--accent);" />
				Releases
				<ArrowUpRight class="h-3.5 w-3.5 opacity-60" />
			</button>

			<button
				type="button"
				onclick={() => api.openExternalUrl(ISSUES_URL)}
				class="wb-btn"
			>
				<Bug class="h-3.5 w-3.5" style="color: var(--accent);" />
				Report an Issue
				<ArrowUpRight class="h-3.5 w-3.5 opacity-60" />
			</button>

			<button
				type="button"
				onclick={() => api.openExternalUrl(`${REPO_URL}/blob/main/SECURITY.md`)}
				class="wb-btn"
			>
				<ShieldCheck class="h-3.5 w-3.5" style="color: var(--accent);" />
				Security Policy
				<ArrowUpRight class="h-3.5 w-3.5 opacity-60" />
			</button>
		</div>
	</section>
</div>
