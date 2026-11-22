<script lang="ts">
	import {
		siApache,
		siCaddy,
		siComposer,
		siFlutter,
		siLaravel,
		siMariadb,
		siMysql,
		siNodedotjs,
		siPhp,
		siPhpmyadmin,
		siPostgresql,
		siRedis,
		siRust,
		siSvelte,
		type SimpleIcon
	} from 'simple-icons';

	interface Props {
		name: string;
		class?: string;
	}

	let { name, class: className = 'h-4 w-4' }: Props = $props();

	const iconMap: Record<string, { icon: SimpleIcon; color: string }> = {
		laravel: { icon: siLaravel, color: `#${siLaravel.hex}` },
		php: { icon: siPhp, color: '#9BA3DC' },
		sveltekit: { icon: siSvelte, color: `#${siSvelte.hex}` },
		svelte: { icon: siSvelte, color: `#${siSvelte.hex}` },
		node: { icon: siNodedotjs, color: '#6CC24A' },
		nodejs: { icon: siNodedotjs, color: '#6CC24A' },
		npm: { icon: siNodedotjs, color: '#6CC24A' },
		flutter: { icon: siFlutter, color: '#54C5F8' },
		dart: { icon: siFlutter, color: '#54C5F8' },
		rust: { icon: siRust, color: '#F74C00' },
		cargo: { icon: siRust, color: '#F74C00' },
		composer: { icon: siComposer, color: '#D99B52' },
		mariadb: { icon: siMariadb, color: '#4EA8DE' },
		mysql: { icon: siMysql, color: '#4479A1' },
		postgresql: { icon: siPostgresql, color: '#4169E1' },
		postgres: { icon: siPostgresql, color: '#4169E1' },
		redis: { icon: siRedis, color: `#${siRedis.hex}` },
		caddy: { icon: siCaddy, color: '#29B6F6' },
		apache: { icon: siApache, color: '#E84545' },
		phpmyadmin: { icon: siPhpmyadmin, color: '#F89C0E' }
	};

	const entry = $derived(iconMap[name.toLowerCase()] ?? null);
</script>

{#if entry}
	<svg
		viewBox="0 0 24 24"
		fill={entry.color}
		class={className}
		role="img"
		aria-label={entry.icon.title}
	>
		<path d={entry.icon.path} />
	</svg>
{:else}
	<svg
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="1.75"
		stroke-linecap="round"
		stroke-linejoin="round"
		class={className}
		aria-hidden="true"
	>
		<path d="M8 8L4 12L8 16M16 8L20 12L16 16M13.5 5.5L10.5 18.5" />
	</svg>
{/if}
