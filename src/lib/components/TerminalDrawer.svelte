<script lang="ts">
	import { wb } from '$lib/stores/workbench.svelte';
	import { X, Terminal } from 'lucide-svelte';

	let scrollEl: HTMLDivElement | undefined = $state();

	$effect(() => {
		const _len = wb.terminalLogs.length;
		if (scrollEl) {
			scrollEl.scrollTop = scrollEl.scrollHeight;
		}
	});
</script>

{#if wb.terminalOpen}
	<div
		class="flex h-56 shrink-0 flex-col border-t"
		style="background-color: var(--bg-sidebar); border-color: var(--border-strong); color: var(--text-primary);"
	>
		<div
			class="flex items-center justify-between border-b px-4 py-2 text-xs"
			style="border-color: var(--border-subtle); background-color: var(--bg-surface);"
		>
			<div class="flex items-center gap-2 font-medium min-w-0">
				<Terminal class="h-3.5 w-3.5 shrink-0" style="color: var(--accent);" />
				<span class="shrink-0">Wake Output</span>
				{#if wb.terminalProject}
					<span class="truncate font-mono text-[11px]" style="color: var(--text-muted);">
						{wb.terminalProject}
					</span>
				{/if}
				{#if wb.terminalRunning}
					<span
						class="shrink-0 rounded-full px-2 py-0.5 font-mono text-[10px] font-medium"
						style="background-color: var(--accent-soft); color: var(--accent);"
					>
						Running
					</span>
				{/if}
			</div>
			<button
				type="button"
				aria-label="Close terminal drawer"
				onclick={() => (wb.terminalOpen = false)}
				class="rounded-full p-1 transition hover:bg-[var(--bg-surface-hover)]"
				style="color: var(--text-muted);"
			>
				<X class="h-3.5 w-3.5" />
			</button>
		</div>
		<div
			bind:this={scrollEl}
			class="selectable flex-1 overflow-y-auto px-4 py-2.5 font-mono text-[12px] leading-relaxed"
			style="background-color: var(--bg-app);"
		>
			{#if wb.terminalLogs.length === 0}
				<div style="color: var(--text-muted);">Starting restore commands...</div>
			{:else}
				{#each wb.terminalLogs as entry}
					<div
						style="color: {entry.stream === 'stderr'
							? 'var(--warning)'
							: entry.stream === 'info'
								? 'var(--accent)'
								: 'var(--text-primary)'};"
					>
						{entry.line}
					</div>
				{/each}
			{/if}
		</div>
	</div>
{/if}
