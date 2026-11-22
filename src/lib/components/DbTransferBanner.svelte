<script lang="ts">
	import { api } from '$lib/api';
	import { wb } from '$lib/stores/workbench.svelte';
	import { formatBytes } from '$lib/format';
	import { AlertCircle, AlertTriangle, CheckCircle2, FolderOpen, RefreshCw, X } from 'lucide-svelte';

	const transfer = $derived(wb.dbTransfer);
	const isRunning = $derived(transfer?.stage === 'running');
	const isDone = $derived(transfer?.stage === 'done');
	const isWarning = $derived(transfer?.stage === 'warning');
	const isError = $derived(transfer?.stage === 'error');
	const hasTotal = $derived((transfer?.total_bytes ?? 0) > 0);

	async function revealFile(path: string) {
		try {
			await api.revealInFileManager(path);
		} catch (e) {
			wb.showToast(`Could not open file location: ${e}`, 'error');
		}
	}
</script>

{#if transfer}
	<div
		class="db-transfer-banner rounded-2xl p-4"
		style="background-color: {isError
			? 'color-mix(in srgb, var(--danger) 10%, var(--bg-surface))'
			: isWarning
				? 'color-mix(in srgb, var(--warning) 10%, var(--bg-surface))'
				: isDone
					? 'color-mix(in srgb, var(--success) 10%, var(--bg-surface))'
					: 'color-mix(in srgb, var(--accent) 8%, var(--bg-surface))'};"
		role="status"
		aria-live="polite"
	>
		<div class="flex items-start justify-between gap-3">
			<div class="flex min-w-0 flex-1 items-start gap-3">
				<div
					class="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-xl"
					style="background-color: {isError
						? 'color-mix(in srgb, var(--danger) 18%, transparent)'
						: isWarning
							? 'color-mix(in srgb, var(--warning) 18%, transparent)'
							: isDone
								? 'color-mix(in srgb, var(--success) 18%, transparent)'
								: 'color-mix(in srgb, var(--accent) 16%, transparent)'}; color: {isError
						? 'var(--danger)'
						: isWarning
							? 'var(--warning)'
							: isDone
								? 'var(--success)'
								: 'var(--accent)'};"
				>
					{#if isRunning}
						<RefreshCw class="h-4 w-4 animate-spin" />
					{:else if isWarning}
						<AlertTriangle class="h-4 w-4" />
					{:else if isDone}
						<CheckCircle2 class="h-4 w-4" />
					{:else}
						<AlertCircle class="h-4 w-4" />
					{/if}
				</div>

				<div class="min-w-0 flex-1 space-y-1.5">
					<div class="flex flex-wrap items-center justify-between gap-2">
						<div class="flex min-w-0 items-center gap-2">
							<span class="text-xs font-semibold" style="color: var(--text-primary);">
								{#if isRunning}
									{transfer.op === 'export' ? 'Exporting' : 'Importing'}
									<span class="font-mono">{transfer.target}</span>
								{:else if isWarning}
									{transfer.op === 'export' ? 'Export complete (skipped broken objects)' : 'Import complete (with warnings)'} —
									<span class="font-mono">{transfer.target}</span>
								{:else if isDone}
									{transfer.op === 'export' ? 'Export complete' : 'Import complete'} —
									<span class="font-mono">{transfer.target}</span>
								{:else}
									{transfer.op === 'export' ? 'Export failed' : 'Import failed'} —
									<span class="font-mono">{transfer.target}</span>
								{/if}
							</span>
						</div>

						{#if isRunning}
							<span class="font-mono text-[11px]" style="color: var(--text-secondary);">
								{#if hasTotal}
									{transfer.percent}% · {formatBytes(transfer.bytes_processed)} / {formatBytes(transfer.total_bytes)}
								{:else if transfer.bytes_processed > 0}
									{formatBytes(transfer.bytes_processed)} streamed
								{:else}
									Starting...
								{/if}
							</span>
						{:else if (isDone || isWarning) && transfer.bytes_processed > 0}
							<span class="font-mono text-[11px]" style="color: var(--text-secondary);">
								{formatBytes(transfer.bytes_processed)}
							</span>
						{/if}
					</div>

					{#if isRunning}
						<div
							class="h-1.5 w-full overflow-hidden rounded-full"
							style="background-color: color-mix(in srgb, var(--text-muted) 18%, transparent);"
						>
							{#if hasTotal}
								<div
									class="h-full rounded-full transition-[width] duration-150"
									style="width: {Math.max(4, Math.min(100, transfer.percent))}%; background-color: var(--accent);"
								></div>
							{:else}
								<div class="db-indeterminate-bar h-full rounded-full"></div>
							{/if}
						</div>
					{/if}

					{#if isError}
						<pre
							class="max-h-32 overflow-auto whitespace-pre-wrap break-words rounded-xl p-2.5 font-mono text-[11px] leading-relaxed select-text"
							style="background-color: var(--bg-app); color: var(--danger);"
						>{transfer.message}</pre>
					{:else if isWarning}
						{#if transfer.file_path}
							<p class="truncate font-mono text-[11px]" style="color: var(--text-secondary);">
								Saved to {transfer.file_path}
							</p>
						{/if}
						<pre
							class="max-h-32 overflow-auto whitespace-pre-wrap break-words rounded-xl p-2.5 font-mono text-[11px] leading-relaxed select-text"
							style="background-color: var(--bg-app); color: var(--warning);"
						>{transfer.message}</pre>
					{:else if isDone}
						<p class="truncate font-mono text-[11px]" style="color: var(--text-secondary);">
							{transfer.message}{#if transfer.file_path} · {transfer.file_path}{/if}
						</p>
					{:else if transfer.file_path}
						<p class="truncate font-mono text-[11px]" style="color: var(--text-muted);">
							{transfer.file_path}
						</p>
					{/if}
				</div>
			</div>

			<div class="flex shrink-0 items-center gap-1.5">
				{#if (isDone || isWarning) && transfer.op === 'export' && transfer.file_path}
					<button
						type="button"
						onclick={() => transfer.file_path && revealFile(transfer.file_path)}
						class="wb-btn"
					>
						<FolderOpen class="h-3.5 w-3.5" />
						Reveal
					</button>
				{/if}

				{#if !isRunning}
					<button
						type="button"
						onclick={() => wb.clearDbTransfer()}
						class="flex h-7 w-7 items-center justify-center rounded-full transition hover:bg-[var(--bg-surface-hover)]"
						style="color: var(--text-muted);"
						title="Dismiss"
						aria-label="Dismiss transfer status"
					>
						<X class="h-3.5 w-3.5" />
					</button>
				{/if}
			</div>
		</div>
	</div>
{/if}

<style>
	.db-indeterminate-bar {
		width: 38%;
		background-color: var(--accent);
		animation: db-slide 1.25s ease-in-out infinite;
	}

	@keyframes db-slide {
		0% {
			transform: translateX(-110%);
		}
		100% {
			transform: translateX(300%);
		}
	}
</style>
