<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { listen } from '@tauri-apps/api/event';
	import { wb } from '$lib/stores/workbench.svelte';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import TerminalDrawer from '$lib/components/TerminalDrawer.svelte';
	import CommandPalette from '$lib/components/CommandPalette.svelte';

	let { children } = $props();
	let paletteOpen = $state(false);

	const shortcutRoutes: Record<string, string> = {
		'1': '/',
		'2': '/projects',
		'3': '/domains',
		'4': '/databases',
		'5': '/services',
		'6': '/runtimes',
		'7': '/storage',
		'8': '/settings',
		'9': '/about'
	};

	onMount(() => {
		wb.init();

		const onKey = (e: KeyboardEvent) => {
			if (!(e.metaKey || e.ctrlKey)) return;
			const key = e.key.toLowerCase();
			if (key === 'k') {
				e.preventDefault();
				paletteOpen = !paletteOpen;
			} else if (key === 'r') {
				e.preventDefault();
				wb.scanProjects();
				wb.refreshCaches();
			} else if (shortcutRoutes[key]) {
				e.preventDefault();
				goto(shortcutRoutes[key]);
			}
		};
		window.addEventListener('keydown', onKey);
		let unlistenTrayNav: (() => void) | null = null;
		void listen<string>('tray-navigate', (ev) => {
			if (typeof ev.payload === 'string' && ev.payload.startsWith('/')) {
				void goto(ev.payload);
			}
		})
			.then((fn) => {
				unlistenTrayNav = fn;
			})
			.catch(() => {});

		return () => {
			window.removeEventListener('keydown', onKey);
			unlistenTrayNav?.();
			wb.destroy();
		};
	});
</script>

<div class="flex h-[100dvh] w-[100dvw] overflow-hidden" style="background-color: var(--bg-app);">
	<Sidebar onOpenPalette={() => (paletteOpen = true)} />

	<div class="flex min-w-0 flex-1 flex-col overflow-hidden">
		<!-- Comfortable 36px native window drag region across the top edge -->
		<div data-tauri-drag-region class="h-9 w-full shrink-0"></div>

		<main class="min-h-0 flex-1 overflow-y-auto px-[clamp(1rem,2.4dvw,2.25rem)] pb-8 pt-0">
			{@render children()}
		</main>

		<TerminalDrawer />
	</div>

	<CommandPalette bind:open={paletteOpen} />

	{#if wb.toastMessage}
		<div
			role="status"
			class="fixed bottom-4 right-4 z-50 flex items-center gap-2.5 rounded-[12px] border px-4 py-2.5 text-xs font-medium"
			style="
				background-color: var(--bg-surface);
				border-color: var(--border-subtle);
				box-shadow: var(--shadow-dialog);
				color: var(--text-primary);
			"
		>
			<span
				class="h-2 w-2 shrink-0 rounded-full"
				style="background-color: {wb.toastKind === 'error'
					? 'var(--danger)'
					: wb.toastKind === 'success'
						? 'var(--success)'
						: 'var(--accent)'};"
			></span>
			<span>{wb.toastMessage}</span>
		</div>
	{/if}
</div>
