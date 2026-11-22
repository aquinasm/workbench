import { api } from '$lib/api';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type {
	CleanupStats,
	DbTransferProgressEvent,
	DeleteMode,
	GlobalCacheInfo,
	LocalSite,
	NetworkStatus,
	ProjectInfo,
	RuntimeProgressEvent,
	RuntimeVersionEntry,
	RustupStatus,
	ServiceProgressEvent,
	ServiceStatusInfo,
	ShellIntegrationStatus,
	UpdateCheckResult,
	WakeLogEvent,
	WorkbenchConfig
} from '$lib/types';

class WorkbenchStore {
	config = $state<WorkbenchConfig>({
		scan_roots: [],
		delete_mode: 'permanent',
		stale_days: 14,
		theme: 'system'
	});

	projects = $state<ProjectInfo[]>([]);
	totalReclaimable = $state(0);
	lastScanMs = $state<number | null>(null);
	scanningProjects = $state(false);

	caches = $state<GlobalCacheInfo[]>([]);
	scanningCaches = $state(false);

	stats = $state<CleanupStats>({
		total_bytes_reclaimed: 0,
		total_cleanups: 0,
		recent: []
	});

	shellStatus = $state<ShellIntegrationStatus | null>(null);

	// Phase 2: Multi-version runtimes & rustup
	runtimes = $state<RuntimeVersionEntry[]>([]);
	loadingRuntimes = $state(false);
	downloadProgress = $state<Record<string, RuntimeProgressEvent>>({});
	rustup = $state<RustupStatus>({ available: false, toolchains: [] });
	loadingRustup = $state(false);

	// Phase 3: Local services supervisor
	services = $state<ServiceStatusInfo[]>([]);
	loadingServices = $state(false);
	serviceProgress = $state<Record<string, ServiceProgressEvent>>({});
	serviceLogs = $state<Record<string, string[]>>({});

	// Wake terminal drawer state
	terminalOpen = $state(false);
	terminalRunning = $state(false);
	terminalProject = $state<string | null>(null);
	terminalLogs = $state<WakeLogEvent[]>([]);

	private unlisteners: UnlistenFn[] = [];
	private scanInFlight = 0;

	// Toast feedback
	toastMessage = $state<string | null>(null);
	toastKind = $state<'info' | 'success' | 'error'>('info');
	private toastTimer: ReturnType<typeof setTimeout> | null = null;

	// GitHub Releases update status
	updateInfo = $state<UpdateCheckResult>({
		current_version: '0.1.0',
		latest_version: null,
		update_available: false,
		release_url: null,
		checked: false,
		error: null
	});
	checkingUpdates = $state(false);

	// Local DNS, SSL & LAN network status
	networkStatus = $state<NetworkStatus | null>(null);
	syncingDns = $state(false);
	trustingCa = $state(false);

	// Database export / import live progress & result banner
	dbTransfer = $state<DbTransferProgressEvent | null>(null);

	initialized = $state(false);

	clearDbTransfer() {
		this.dbTransfer = null;
	}

	showToast(message: string, kind: 'info' | 'success' | 'error' = 'info') {
		this.toastMessage = message;
		this.toastKind = kind;
		if (this.toastTimer) clearTimeout(this.toastTimer);
		this.toastTimer = setTimeout(() => {
			this.toastMessage = null;
		}, 4200);
	}

	applyTheme(theme: string) {
		if (typeof document === 'undefined') return;
		const root = document.documentElement;
		if (theme === 'light' || theme === 'dark') {
			root.setAttribute('data-theme', theme);
		} else {
			root.removeAttribute('data-theme');
		}
	}

	destroy() {
		for (const unlisten of this.unlisteners) {
			try {
				unlisten();
			} catch {
				// Ignore
			}
		}
		this.unlisteners = [];
		if (this.toastTimer) {
			clearTimeout(this.toastTimer);
			this.toastTimer = null;
		}
		this.initialized = false;
	}

	async init() {
		if (this.initialized) return;
		this.initialized = true;

		// Attach live event listeners immediately and retain UnlistenFn handles for cleanup
		void Promise.all([
			api.onDbTransferProgress((ev) => {
				this.dbTransfer = ev;
			}),
			api.onServiceProgress((ev) => {
				if (ev.stage === 'done') {
					const next = { ...this.serviceProgress };
					delete next[ev.service_id];
					this.serviceProgress = next;
				} else {
					this.serviceProgress = { ...this.serviceProgress, [ev.service_id]: ev };
				}
			}),
			api.onServiceLog((ev) => {
				const existing = this.serviceLogs[ev.service_id] ?? [];
				this.serviceLogs = {
					...this.serviceLogs,
					[ev.service_id]: [...existing.slice(-249), ev.line]
				};
			}),
			api.onRuntimeProgress((ev) => {
				const key = `${ev.tool}:${ev.version}`;
				if (ev.stage === 'done') {
					const next = { ...this.downloadProgress };
					delete next[key];
					this.downloadProgress = next;
				} else {
					this.downloadProgress = { ...this.downloadProgress, [key]: ev };
				}
			}),
			api.onWakeLog((ev) => {
				if (this.terminalLogs.length >= 1000) {
					this.terminalLogs.splice(0, this.terminalLogs.length - 999);
				}
				this.terminalLogs.push(ev);
			}),
			api.onWakeDone(async (ev) => {
				this.terminalRunning = false;
				if (this.terminalLogs.length >= 1000) {
					this.terminalLogs.splice(0, this.terminalLogs.length - 999);
				}
				this.terminalLogs.push({
					project_path: ev.project_path,
					line: ev.message,
					stream: ev.success ? 'info' : 'stderr'
				});
				this.showToast(ev.message, ev.success ? 'success' : 'error');
				await this.scanProjects();
			})
		])
			.then((fns) => {
				this.unlisteners = fns;
			})
			.catch(() => {
				// Ignore listener attach failures in non-Tauri environments
			});

		// Load config in parallel with independent lightweight metadata
		const configPromise = api
			.getConfig()
			.then((cfg) => {
				this.config = cfg;
				this.applyTheme(cfg.theme);
			})
			.catch((e) => {
				this.showToast(`Failed to load config: ${e}`, 'error');
			});

		await Promise.all([
			configPromise.then(() =>
				this.config.scan_roots.length > 0 ? this.discoverProjectsFast() : Promise.resolve()
			),
			this.refreshStats(),
			this.refreshShellStatus(),
			this.refreshServices()
		]);

		// Stage 2: Heavy disk-size & toolchain scans run in background without blocking initial render
		void Promise.all([
			this.config.scan_roots.length > 0 ? this.scanProjects() : Promise.resolve(),
			this.refreshRuntimes(),
			this.refreshRustup(),
			this.refreshCaches(),
			this.checkForUpdates(false),
			this.refreshNetworkStatus()
		]);
	}

	async refreshNetworkStatus() {
		try {
			this.networkStatus = await api.getNetworkStatus();
		} catch {
			// non-blocking
		}
	}

	async syncHostsDns() {
		this.syncingDns = true;
		try {
			this.networkStatus = await api.syncHostsFile();
			this.showToast('Local .test DNS entries synced to hosts file', 'success');
		} catch (e) {
			this.showToast(`DNS sync failed: ${e}`, 'error');
		} finally {
			this.syncingDns = false;
		}
	}

	async trustLocalCa() {
		this.trustingCa = true;
		try {
			const msg = await api.trustLocalCa();
			this.showToast(msg, 'success');
		} catch (e) {
			this.showToast(`Trust CA failed: ${e}`, 'error');
		} finally {
			this.trustingCa = false;
		}
	}

	async checkForUpdates(manual = false) {
		this.checkingUpdates = true;
		try {
			const res = await api.checkForUpdates();
			this.updateInfo = res;
			if (manual) {
				if (res.update_available && res.latest_version) {
					this.showToast(`Workbench v${res.latest_version} is available`, 'info');
				} else if (res.checked) {
					this.showToast(`You're on the latest version (v${res.current_version})`, 'success');
				} else if (res.error) {
					this.showToast(res.error, 'error');
				}
			}
		} catch (e) {
			if (manual) {
				this.showToast(`Update check failed: ${e}`, 'error');
			}
		} finally {
			this.checkingUpdates = false;
		}
	}

	async discoverProjectsFast() {
		this.scanInFlight++;
		this.scanningProjects = true;
		try {
			const res = await api.discoverProjectsFast();
			// Only populate if full scan hasn't already completed with exact sizes
			if (this.projects.length === 0 || this.totalReclaimable === 0) {
				this.projects = res.projects;
			}
		} catch (e) {
			this.showToast(`Discovery failed: ${e}`, 'error');
		} finally {
			this.scanInFlight = Math.max(0, this.scanInFlight - 1);
			this.scanningProjects = this.scanInFlight > 0;
		}
	}

	async scanProjects() {
		this.scanInFlight++;
		this.scanningProjects = true;
		try {
			const res = await api.scanProjects();
			this.projects = res.projects;
			this.totalReclaimable = res.total_reclaimable_bytes;
			this.lastScanMs = res.scan_duration_ms;
		} catch (e) {
			this.showToast(`Scan failed: ${e}`, 'error');
		} finally {
			this.scanInFlight = Math.max(0, this.scanInFlight - 1);
			this.scanningProjects = this.scanInFlight > 0;
		}
	}

	async refreshCaches() {
		this.scanningCaches = true;
		try {
			this.caches = await api.scanGlobalCaches();
		} catch (e) {
			this.showToast(`Failed scanning caches: ${e}`, 'error');
		} finally {
			this.scanningCaches = false;
		}
	}

	async refreshStats() {
		try {
			this.stats = await api.getCleanupStats();
		} catch {
			// Ignore on fresh install
		}
	}

	async refreshShellStatus() {
		try {
			this.shellStatus = await api.getShellStatus();
		} catch {
			// Ignore
		}
	}

	async addWorkspaceFolder() {
		try {
			const folder = await api.pickFolder();
			if (!folder) return;
			this.config = await api.addScanRoot(folder);
			this.showToast(`Added workspace: ${folder}`, 'success');
			await this.discoverProjectsFast();
			void this.scanProjects();
		} catch (e) {
			this.showToast(`Could not add folder: ${e}`, 'error');
		}
	}

	async removeWorkspaceFolder(path: string) {
		try {
			this.config = await api.removeScanRoot(path);
			this.showToast('Removed workspace folder', 'info');
			if (this.config.scan_roots.length === 0) {
				this.projects = [];
				this.totalReclaimable = 0;
				return;
			}
			await this.discoverProjectsFast();
			void this.scanProjects();
		} catch (e) {
			this.showToast(`${e}`, 'error');
		}
	}

	async saveConfig(partial: Partial<WorkbenchConfig>) {
		const next: WorkbenchConfig = { ...this.config, ...partial };
		try {
			this.config = await api.updateConfig(next);
			this.applyTheme(this.config.theme);
			this.showToast('Preferences saved', 'success');
		} catch (e) {
			this.showToast(`Failed saving config: ${e}`, 'error');
		}
	}

	async hibernateOne(projectPath: string, selectedArtifacts?: string[], mode?: DeleteMode) {
		try {
			const res = await api.hibernateProject(projectPath, selectedArtifacts, mode);
			if (res.errors.length > 0) {
				this.showToast(`Hibernated with warnings: ${res.errors[0]}`, 'error');
			} else {
				this.showToast(`Hibernated ${res.project_name}`, 'success');
			}
			await Promise.all([this.scanProjects(), this.refreshStats()]);
			return res;
		} catch (e) {
			this.showToast(`Hibernate failed: ${e}`, 'error');
			return null;
		}
	}

	async hibernateBulk(projectPaths: string[], mode?: DeleteMode) {
		try {
			const results = await api.hibernateProjectsBulk(projectPaths, mode);
			const total = results.reduce((sum, r) => sum + r.bytes_reclaimed, 0);
			this.showToast(`Hibernated ${results.length} projects`, 'success');
			await Promise.all([this.scanProjects(), this.refreshStats()]);
			return { results, total };
		} catch (e) {
			this.showToast(`Bulk hibernate failed: ${e}`, 'error');
			return null;
		}
	}

	async wakeOne(projectPath: string) {
		if (this.terminalRunning) {
			this.terminalOpen = true;
			this.showToast('Another project wake is already running', 'info');
			return;
		}
		this.terminalProject = projectPath;
		this.terminalLogs = [];
		this.terminalOpen = true;
		this.terminalRunning = true;
		try {
			await api.wakeProject(projectPath);
		} catch (e) {
			this.terminalRunning = false;
			this.terminalLogs = [
				{ project_path: projectPath, line: `${e}`, stream: 'stderr' }
			];
			this.showToast(`Wake failed: ${e}`, 'error');
		}
	}

	async cleanCache(cacheId: string, mode?: DeleteMode) {
		try {
			await api.cleanGlobalCache(cacheId, mode);
			this.showToast(`Cleaned ${cacheId} cache`, 'success');
			await Promise.all([this.refreshCaches(), this.refreshStats()]);
		} catch (e) {
			this.showToast(`Clean cache failed: ${e}`, 'error');
		}
	}

	// --- Phase 2: Runtimes, Shims, Rustup & Project Pins ---

	async refreshRuntimes() {
		this.loadingRuntimes = true;
		try {
			this.runtimes = await api.listRuntimes();
		} catch (e) {
			this.showToast(`Failed loading runtimes: ${e}`, 'error');
		} finally {
			this.loadingRuntimes = false;
		}
	}

	async installRuntime(tool: string, version: string) {
		try {
			await api.installRuntime(tool, version);
			this.showToast(`Installed ${tool} ${version}`, 'success');
			await this.refreshRuntimes();
		} catch (e) {
			const key = `${tool}:${version}`;
			const next = { ...this.downloadProgress };
			delete next[key];
			this.downloadProgress = next;
			this.showToast(`Install failed: ${e}`, 'error');
		}
	}

	async uninstallRuntime(tool: string, version: string) {
		try {
			await api.uninstallRuntime(tool, version);
			this.showToast(`Uninstalled ${tool} ${version}`, 'info');
			await Promise.all([this.refreshRuntimes(), this.refreshStats()]);
		} catch (e) {
			this.showToast(`Uninstall failed: ${e}`, 'error');
		}
	}

	async setDefaultRuntime(tool: string, version: string) {
		try {
			await api.setDefaultRuntime(tool, version);
			this.showToast(`Default ${tool} set to ${version}`, 'success');
			await this.refreshRuntimes();
		} catch (e) {
			this.showToast(`Failed setting default: ${e}`, 'error');
		}
	}

	async syncShims() {
		try {
			const report = await api.syncShims();
			this.showToast(
				`Synced ${report.shims_created.length} shims (${report.mode})`,
				'success'
			);
			return report;
		} catch (e) {
			this.showToast(`Shim sync failed: ${e}`, 'error');
			return null;
		}
	}

	async refreshRustup() {
		this.loadingRustup = true;
		try {
			this.rustup = await api.getRustupStatus();
		} catch {
			// Ignore if rustup is absent
		} finally {
			this.loadingRustup = false;
		}
	}

	async setProjectPin(projectPath: string, tool: string, version: string | null) {
		try {
			const updatedPins = await api.setProjectPin(projectPath, tool, version);
			this.projects = this.projects.map((p) =>
				p.path === projectPath
					? { ...p, version_hints: { ...p.version_hints, ...updatedPins } }
					: p
			);
			this.showToast(
				version ? `Pinned ${tool} ${version} in .workbench.toml` : `Cleared ${tool} pin`,
				'success'
			);
		} catch (e) {
			this.showToast(`Failed updating pin: ${e}`, 'error');
		}
	}

	// --- Phase 3: Services & Local Sites ---

	async refreshServices() {
		this.loadingServices = true;
		try {
			this.services = await api.listServices();
		} catch (e) {
			this.showToast(`Failed loading services: ${e}`, 'error');
		} finally {
			this.loadingServices = false;
		}
	}

	async installService(serviceId: string) {
		try {
			await api.installService(serviceId);
			this.showToast(`Installed ${serviceId}`, 'success');
			await this.refreshServices();
		} catch (e) {
			const next = { ...this.serviceProgress };
			delete next[serviceId];
			this.serviceProgress = next;
			this.showToast(`Install failed: ${e}`, 'error');
		}
	}

	async uninstallService(serviceId: string) {
		try {
			await api.uninstallService(serviceId);
			this.showToast(`Uninstalled managed ${serviceId}`, 'info');
			await this.refreshServices();
		} catch (e) {
			this.showToast(`Uninstall failed: ${e}`, 'error');
		}
	}

	async startService(serviceId: string, port?: number) {
		try {
			await api.startService(serviceId, port);
			this.showToast(`Started ${serviceId}`, 'success');
			await this.refreshServices();
		} catch (e) {
			this.showToast(`${e}`, 'error');
		}
	}

	async stopService(serviceId: string) {
		try {
			await api.stopService(serviceId);
			this.showToast(`Stopped ${serviceId} gracefully`, 'info');
			await this.refreshServices();
		} catch (e) {
			this.showToast(`${e}`, 'error');
		}
	}

	async restartService(serviceId: string, port?: number) {
		try {
			await api.restartService(serviceId, port);
			this.showToast(`Restarted ${serviceId}`, 'success');
			await this.refreshServices();
		} catch (e) {
			this.showToast(`${e}`, 'error');
		}
	}

	async startAllServices() {
		try {
			const started = await api.startAllServices();
			this.showToast(
				started.length > 0
					? `Started ${started.length} ${started.length === 1 ? 'service' : 'services'}`
					: 'All available services are already running',
				'success'
			);
			await this.refreshServices();
		} catch (e) {
			this.showToast(`${e}`, 'error');
		}
	}

	async stopAllServices() {
		try {
			await api.stopAllServices();
			this.showToast('Stopped all services gracefully', 'info');
			await this.refreshServices();
		} catch (e) {
			this.showToast(`${e}`, 'error');
		}
	}

	async addLocalSite(site: LocalSite) {
		try {
			const sites = await api.addLocalSite(site);
			this.config = { ...this.config, sites };
			this.showToast(`Linked ${site.domain}`, 'success');
		} catch (e) {
			this.showToast(`Failed linking site: ${e}`, 'error');
		}
	}

	async removeLocalSite(domain: string) {
		try {
			const sites = await api.removeLocalSite(domain);
			this.config = { ...this.config, sites };
			this.showToast(`Unlinked ${domain}`, 'info');
		} catch (e) {
			this.showToast(`Failed removing site: ${e}`, 'error');
		}
	}
}

export const wb = new WorkbenchStore();
