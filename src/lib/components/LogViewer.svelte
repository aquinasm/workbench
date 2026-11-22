<script lang="ts">
	import { api } from '$lib/api';
	import { RefreshCw, FolderOpen, Copy, X, Search } from 'lucide-svelte';

	interface Props {
		title: string;
		lines: string[];
		sourcePath?: string | null;
		loading?: boolean;
		onRefresh?: () => void;
		onClose?: () => void;
	}

	let {
		title,
		lines,
		sourcePath = null,
		loading = false,
		onRefresh,
		onClose
	}: Props = $props();

	let levelFilter = $state<'all' | 'error' | 'warn' | 'info'>('all');
	let searchQuery = $state('');
	let copied = $state(false);
	let scrollEl: HTMLDivElement | undefined = $state();

	type LogLevel = 'error' | 'warn' | 'info' | 'http' | 'plain';

	interface ParsedLine {
		idx: number;
		level: LogLevel;
		timestamp: string | null;
		badge: string | null;
		message: string;
		raw: string;
	}

	function parseLogLine(raw: string, idx: number): ParsedLine {
		const lower = raw.toLowerCase();

		// Extract leading timestamp (ISO, MariaDB YYYY-MM-DD HH:MM:SS, Redis, Apache [Date])
		let timestamp: string | null = null;
		let rest = raw;
		const tsMatch = raw.match(
			/^(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?|\d+:[A-Z]\s+\d+\s+[A-Za-z]+\s+\d{4}\s+\d{2}:\d{2}:\d{2}\.\d+|\[[A-Z][a-z]{2}\s+[A-Z][a-z]{2}\s+\d+\s+\d{2}:\d{2}:\d{2}[^\]]*\])\s*(.*)$/
		);
		if (tsMatch) {
			timestamp = tsMatch[1];
			rest = tsMatch[2];
		}

		// Classify severity
		let level: LogLevel = 'plain';
		let badge: string | null = null;
		if (
			lower.includes('[error]') ||
			lower.includes('error:') ||
			lower.includes('fatal') ||
			lower.includes('emerg') ||
			lower.includes('crit') ||
			lower.includes('exception') ||
			/\s5\d{2}\s/.test(raw)
		) {
			level = 'error';
			badge = 'ERR';
		} else if (
			lower.includes('[warning]') ||
			lower.includes('warn') ||
			/\s4\d{2}\s/.test(raw)
		) {
			level = 'warn';
			badge = 'WARN';
		} else if (
			/\b(GET|POST|PUT|PATCH|DELETE|HEAD)\b/.test(raw) ||
			/\s2\d{2}\s/.test(raw) ||
			/\s3\d{2}\s/.test(raw)
		) {
			level = 'http';
			badge = 'HTTP';
		} else if (
			lower.includes('[note]') ||
			lower.includes('info') ||
			lower.includes('notice') ||
			lower.includes('ready for connections') ||
			lower.includes('started ') ||
			lower.includes('listening') ||
			lower.includes('server initialized')
		) {
			level = 'info';
			badge = 'INFO';
		}

		return { idx, level, timestamp, badge, message: rest, raw };
	}

	const parsed = $derived(lines.map((l, i) => parseLogLine(l, i)));

	const filtered = $derived.by(() => {
		const q = searchQuery.trim().toLowerCase();
		return parsed.filter((item) => {
			if (levelFilter === 'error' && item.level !== 'error') return false;
			if (levelFilter === 'warn' && item.level !== 'warn') return false;
			if (levelFilter === 'info' && item.level !== 'info' && item.level !== 'http') return false;
			if (q && !item.raw.toLowerCase().includes(q)) return false;
			return true;
		});
	});

	const errorCount = $derived(parsed.filter((p) => p.level === 'error').length);
	const warnCount = $derived(parsed.filter((p) => p.level === 'warn').length);

	$effect(() => {
		const _len = filtered.length;
		if (scrollEl) {
			scrollEl.scrollTop = scrollEl.scrollHeight;
		}
	});

	async function copyAll() {
		try {
			await navigator.clipboard.writeText(filtered.map((f) => f.raw).join('\n'));
			copied = true;
			setTimeout(() => (copied = false), 1500);
		} catch {
			// ignore
		}
	}

	function levelColor(level: LogLevel): string {
		switch (level) {
			case 'error':
				return '#ff5566';
			case 'warn':
				return '#fbbf24';
			case 'info':
				return '#4ade80';
			case 'http':
				return '#38bdf8';
			default:
				return 'var(--text-secondary)';
		}
	}
</script>

<section
	class="overflow-hidden border"
	style="border-color: var(--border-strong); background-color: var(--bg-surface);"
>
	<!-- Header -->
	<div
		class="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-2.5"
		style="border-color: var(--border-subtle); background-color: var(--bg-elevated);"
	>
		<div class="min-w-0">
			<div class="flex flex-wrap items-center gap-2">
				<span class="text-sm font-semibold" style="color: var(--text-primary);">{title}</span>
				<span class="font-mono text-xs" style="color: var(--text-muted);">
					{filtered.length} lines
				</span>
				{#if errorCount > 0}
					<span class="border border-red-500/40 bg-red-500/15 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-red-400">
						{errorCount} err
					</span>
				{/if}
				{#if warnCount > 0}
					<span class="border border-amber-500/40 bg-amber-500/15 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-amber-300">
						{warnCount} warn
					</span>
				{/if}
			</div>
			{#if sourcePath}
				<div class="selectable mt-0.5 truncate font-mono text-[11px]" style="color: var(--text-muted);">
					{sourcePath}
				</div>
			{/if}
		</div>

		<div class="flex flex-wrap items-center gap-1.5">
			{#if sourcePath}
				<button
					type="button"
					title="Reveal log file"
					onclick={() => api.revealInFileManager(sourcePath!)}
					class="wb-btn px-2.5"
				>
					<FolderOpen class="h-3.5 w-3.5" />
				</button>
			{/if}
			<button type="button" onclick={copyAll} class="wb-btn px-2.5">
				<Copy class="h-3.5 w-3.5" />
				{copied ? 'Copied' : 'Copy'}
			</button>
			{#if onRefresh}
				<button type="button" disabled={loading} onclick={onRefresh} class="wb-btn px-2.5">
					<RefreshCw class="h-3.5 w-3.5 {loading ? 'animate-spin' : ''}" />
				</button>
			{/if}
			{#if onClose}
				<button type="button" aria-label="Close logs" onclick={onClose} class="wb-btn px-2">
					<X class="h-3.5 w-3.5" />
				</button>
			{/if}
		</div>
	</div>

	<!-- Filter & Search Bar -->
	<div
		class="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-2"
		style="border-color: var(--border-subtle); background-color: var(--bg-surface);"
	>
		<div class="inline-flex gap-1">
			{#each [
				{ id: 'all', label: 'All' },
				{ id: 'error', label: 'Errors' },
				{ id: 'warn', label: 'Warnings' },
				{ id: 'info', label: 'Info / HTTP' }
			] as tab}
				<button
					type="button"
					onclick={() => (levelFilter = tab.id as typeof levelFilter)}
					class="border px-2.5 py-1 font-mono text-xs transition"
					style="
						border-color: {levelFilter === tab.id ? 'var(--accent)' : 'var(--border-subtle)'};
						background-color: {levelFilter === tab.id ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
						color: {levelFilter === tab.id ? 'var(--text-primary)' : 'var(--text-secondary)'};
					"
				>
					{tab.label}
				</button>
			{/each}
		</div>

		<div class="relative min-w-52 flex-1 max-w-xs">
			<Search
				class="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
				style="color: var(--text-muted);"
			/>
			<input
				type="search"
				bind:value={searchQuery}
				placeholder="Filter log lines..."
				class="w-full border py-1 pl-8 pr-2.5 text-xs outline-none"
				style="border-color: var(--border-subtle); color: var(--text-primary);"
			/>
		</div>
	</div>

	<!-- Colored Log Output -->
	<div
		bind:this={scrollEl}
		class="selectable max-h-80 overflow-y-auto px-3 py-2 font-mono text-[12px] leading-relaxed"
		style="background-color: var(--bg-app);"
	>
		{#if loading && filtered.length === 0}
			<div class="p-4 text-center" style="color: var(--text-muted);">Loading service logs...</div>
		{:else if filtered.length === 0}
			<div class="p-4 text-center" style="color: var(--text-muted);">
				No log lines found. Start this service or adjust filters.
			</div>
		{:else}
			{#each filtered as entry (entry.idx)}
				<div
					class="flex items-start gap-2 rounded-sm px-2 py-0.5 hover:bg-[var(--bg-surface)]"
					style="background-color: {entry.level === 'error'
						? 'rgba(255, 71, 87, 0.08)'
						: entry.level === 'warn'
							? 'rgba(251, 191, 36, 0.06)'
							: 'transparent'};"
				>
					<span class="w-7 shrink-0 select-none text-right text-[11px]" style="color: var(--text-muted);">
						{entry.idx + 1}
					</span>

					{#if entry.badge}
						<span
							class="mt-0.5 w-11 shrink-0 border px-1 text-center text-[10px] font-bold"
							style="border-color: {levelColor(entry.level)}; color: {levelColor(entry.level)};"
						>
							{entry.badge}
						</span>
					{:else}
						<span class="w-11 shrink-0"></span>
					{/if}

					<div class="min-w-0 flex-1 break-all">
						{#if entry.timestamp}
							<span class="mr-2" style="color: #7dd3fc;">{entry.timestamp}</span>
						{/if}
						<span style="color: {entry.level === 'plain' ? 'var(--text-primary)' : levelColor(entry.level)};">
							{entry.message}
						</span>
					</div>
				</div>
			{/each}
		{/if}
	</div>
</section>
