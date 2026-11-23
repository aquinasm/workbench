import type { LocalSite, NetworkStatus, ProjectKind } from './types';

export function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
	const units = ['B', 'KB', 'MB', 'GB', 'TB'];
	const k = 1024;
	const i = Math.max(0, Math.min(Math.floor(Math.log(bytes) / Math.log(k)), units.length - 1));
	const value = bytes / Math.pow(k, i);
	return `${value >= 100 || i === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[i]}`;
}

export function buildSiteUrl(site: LocalSite, network: NetworkStatus | null): string {
	const useHttps = site.https;
	const proto = useHttps ? 'https' : 'http';
	if (network?.portless_enabled) {
		return `${proto}://${site.domain}`;
	}
	const port =
		site.server === 'caddy' || useHttps
			? useHttps
				? (network?.caddy_https_port ?? 8443)
				: (network?.caddy_port ?? 8080)
			: (network?.apache_port ?? 8081);
	const isDefaultPort = (useHttps && port === 443) || (!useHttps && port === 80);
	return isDefaultPort ? `${proto}://${site.domain}` : `${proto}://${site.domain}:${port}`;
}

export function formatIdleDays(days: number): string {
	if (days === 0) return 'Today';
	if (days === 1) return '1d ago';
	if (days < 30) return `${days}d idle`;
	const months = Math.floor(days / 30);
	return months === 1 ? '1mo idle' : `${months}mo idle`;
}

export function kindLabel(kind: ProjectKind): string {
	switch (kind) {
		case 'laravel':
			return 'Laravel';
		case 'php':
			return 'PHP';
		case 'sveltekit':
			return 'SvelteKit';
		case 'node':
			return 'Node';
		case 'flutter':
			return 'Flutter';
		case 'rust':
			return 'Rust';
	}
}

/* Restrained Tron Black & Red badge styling — zero clashing rainbow clutter */
export function kindBadgeClass(kind: ProjectKind): string {
	if (kind === 'laravel' || kind === 'php') {
		return 'border-red-500/30 bg-red-500/10 text-red-500 dark:text-red-400';
	}
	return 'border-zinc-500/25 bg-zinc-500/10 text-zinc-600 dark:text-zinc-300';
}
