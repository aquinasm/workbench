import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
import type {
	CleanupStats,
	DbServerInspection,
	DbServerProfile,
	DbTableInfo,
	DbTablePreview,
	DbTransferProgressEvent,
	DeleteMode,
	DetectedTool,
	GlobalCacheInfo,
	HibernateResult,
	LocalSite,
	NetworkStatus,
	PhpIniSettings,
	PhpVersionDetails,
	RuntimeProgressEvent,
	RuntimeVersionEntry,
	RustupStatus,
	ScanResult,
	ServiceLogEvent,
	ServiceLogsResponse,
	ServiceProgressEvent,
	ServiceStatusInfo,
	ShellIntegrationStatus,
	ShimSyncReport,
	SystemStats,
	UninstallReport,
	UpdateCheckResult,
	WakeDoneEvent,
	WakeLogEvent,
	WorkbenchConfig
} from './types';

export const api = {
	getConfig: () => invoke<WorkbenchConfig>('get_config'),
	updateConfig: (config: WorkbenchConfig) => invoke<WorkbenchConfig>('update_config', { config }),
	addScanRoot: (path: string) => invoke<WorkbenchConfig>('add_scan_root', { path }),
	removeScanRoot: (path: string) => invoke<WorkbenchConfig>('remove_scan_root', { path }),

	discoverProjectsFast: () => invoke<ScanResult>('discover_projects_fast_cmd'),
	scanProjects: () => invoke<ScanResult>('scan_projects'),
	hibernateProject: (
		projectPath: string,
		selectedArtifacts?: string[],
		deleteMode?: DeleteMode
	) =>
		invoke<HibernateResult>('hibernate_project_cmd', {
			projectPath,
			selectedArtifacts: selectedArtifacts ?? null,
			deleteMode: deleteMode ?? null
		}),
	hibernateProjectsBulk: (projectPaths: string[], deleteMode?: DeleteMode) =>
		invoke<HibernateResult[]>('hibernate_projects_bulk', {
			projectPaths,
			deleteMode: deleteMode ?? null
		}),
	wakeProject: (projectPath: string) => invoke<void>('wake_project_cmd', { projectPath }),

	scanGlobalCaches: () => invoke<GlobalCacheInfo[]>('scan_global_caches_cmd'),
	cleanGlobalCache: (cacheId: string, deleteMode?: DeleteMode) =>
		invoke<number>('clean_global_cache_cmd', { cacheId, deleteMode: deleteMode ?? null }),
	getCleanupStats: () => invoke<CleanupStats>('get_cleanup_stats_cmd'),

	getShellStatus: () => invoke<ShellIntegrationStatus>('get_shell_status_cmd'),
	installShellIntegration: () =>
		invoke<ShellIntegrationStatus>('install_shell_integration_cmd'),
	removeShellIntegration: () => invoke<string[]>('remove_shell_integration_cmd'),
	cleanUninstall: () => invoke<UninstallReport>('clean_uninstall_cmd'),

	detectSystemTools: () => invoke<DetectedTool[]>('detect_system_tools_cmd'),
	openInTerminal: (path: string) => invoke<void>('open_in_terminal', { path }),
	revealInFileManager: (path: string) => revealItemInDir(path),

	pickFolder: (): Promise<string | null> => invoke<string | null>('pick_workspace_folder'),

	onWakeLog: (cb: (payload: WakeLogEvent) => void): Promise<UnlistenFn> =>
		listen<WakeLogEvent>('wake-log', (e) => cb(e.payload)),
	onWakeDone: (cb: (payload: WakeDoneEvent) => void): Promise<UnlistenFn> =>
		listen<WakeDoneEvent>('wake-done', (e) => cb(e.payload)),

	// Phase 2: Multi-version runtimes, shims, rustup & .workbench.toml
	listRuntimes: () => invoke<RuntimeVersionEntry[]>('list_runtimes_cmd'),
	installRuntime: (tool: string, version: string) =>
		invoke<RuntimeVersionEntry>('install_runtime_cmd', { tool, version }),
	uninstallRuntime: (tool: string, version: string) =>
		invoke<number>('uninstall_runtime_cmd', { tool, version }),
	setDefaultRuntime: (tool: string, version: string) =>
		invoke<void>('set_default_runtime_cmd', { tool, version }),
	syncShims: () => invoke<ShimSyncReport>('sync_shims_cmd'),

	getRustupStatus: () => invoke<RustupStatus>('get_rustup_status_cmd'),
	installRustupToolchain: (toolchain: string) =>
		invoke<string>('install_rustup_toolchain_cmd', { toolchain }),
	setDefaultRustupToolchain: (toolchain: string) =>
		invoke<string>('set_default_rustup_toolchain_cmd', { toolchain }),
	uninstallRustupToolchain: (toolchain: string) =>
		invoke<string>('uninstall_rustup_toolchain_cmd', { toolchain }),

	getProjectPins: (projectPath: string) =>
		invoke<Record<string, string>>('get_project_pins_cmd', { projectPath }),
	setProjectPin: (projectPath: string, tool: string, version: string | null) =>
		invoke<Record<string, string>>('set_project_pin_cmd', { projectPath, tool, version }),

	onRuntimeProgress: (cb: (payload: RuntimeProgressEvent) => void): Promise<UnlistenFn> =>
		listen<RuntimeProgressEvent>('runtime-progress', (e) => cb(e.payload)),

	// Phase 3: Services supervisor, local sites & phpMyAdmin
	listServices: () => invoke<ServiceStatusInfo[]>('list_services_cmd'),
	installService: (serviceId: string) => invoke<void>('install_service_cmd', { serviceId }),
	startService: (serviceId: string, port?: number) =>
		invoke<ServiceStatusInfo>('start_service_cmd', { serviceId, port: port ?? null }),
	stopService: (serviceId: string) => invoke<void>('stop_service_cmd', { serviceId }),
	restartService: (serviceId: string, port?: number) =>
		invoke<ServiceStatusInfo>('restart_service_cmd', { serviceId, port: port ?? null }),
	startAllServices: () => invoke<string[]>('start_all_services_cmd'),
	stopAllServices: () => invoke<void>('stop_all_services_cmd'),
	getServiceLogs: (serviceId: string) =>
		invoke<ServiceLogsResponse>('get_service_logs_cmd', { serviceId }),
	addLocalSite: (site: LocalSite) => invoke<LocalSite[]>('add_local_site_cmd', { site }),
	removeLocalSite: (domain: string) => invoke<LocalSite[]>('remove_local_site_cmd', { domain }),
	previewWebserverConfig: (server: string) =>
		invoke<string>('preview_webserver_config_cmd', { server }),
	getNetworkStatus: () => invoke<NetworkStatus>('get_network_status_cmd'),
	syncHostsFile: () => invoke<NetworkStatus>('sync_hosts_file_cmd'),
	trustLocalCa: () => invoke<string>('trust_local_ca_cmd'),
	openExternalUrl: (url: string) => {
		let parsed: URL;
		try {
			parsed = new URL(url);
		} catch {
			return Promise.reject(new Error(`Invalid URL: ${url}`));
		}
		if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
			return Promise.reject(new Error(`Blocked unsafe URL protocol: ${parsed.protocol}`));
		}
		return openUrl(parsed.toString());
	},
	onServiceLog: (cb: (payload: ServiceLogEvent) => void): Promise<UnlistenFn> =>
		listen<ServiceLogEvent>('service-log', (e) => cb(e.payload)),
	onServiceProgress: (cb: (payload: ServiceProgressEvent) => void): Promise<UnlistenFn> =>
		listen<ServiceProgressEvent>('service-progress', (e) => cb(e.payload)),

	// PHP Extensions & php.ini
	inspectPhpVersion: (version: string) =>
		invoke<PhpVersionDetails>('inspect_php_version_cmd', { version }),
	togglePhpExtension: (version: string, extName: string, enable: boolean) =>
		invoke<PhpVersionDetails>('toggle_php_extension_cmd', { version, extName, enable }),
	updatePhpIniSettings: (version: string, settings: PhpIniSettings) =>
		invoke<PhpVersionDetails>('update_php_ini_settings_cmd', { version, settings }),

	// Databases management
	saveDbServer: (profile: DbServerProfile) =>
		invoke<DbServerProfile[]>('save_db_server_cmd', { profile }),
	removeDbServer: (serverId: string) =>
		invoke<DbServerProfile[]>('remove_db_server_cmd', { serverId }),
	inspectDbServer: (serverId: string) =>
		invoke<DbServerInspection>('inspect_db_server_cmd', { serverId }),
	listDatabaseTables: (serverId: string, database: string) =>
		invoke<DbTableInfo[]>('list_database_tables_cmd', { serverId, database }),
	inspectTable: (
		serverId: string,
		database: string,
		table: string,
		limit?: number,
		offset?: number
	) =>
		invoke<DbTablePreview>('inspect_table_cmd', {
			serverId,
			database,
			table,
			limit: limit ?? 50,
			offset: offset ?? 0
		}),
	createDatabase: (serverId: string, name: string, charset?: string, collation?: string) =>
		invoke<void>('create_database_cmd', {
			serverId,
			name,
			charset: charset ?? null,
			collation: collation ?? null
		}),
	dropDatabase: (serverId: string, name: string) =>
		invoke<void>('drop_database_cmd', { serverId, name }),
	createDbUser: (serverId: string, username: string, host: string, password: string) =>
		invoke<void>('create_db_user_cmd', { serverId, username, host, password }),
	dropDbUser: (serverId: string, username: string, host: string) =>
		invoke<void>('drop_db_user_cmd', { serverId, username, host }),
	assignUserToDatabase: (
		serverId: string,
		username: string,
		host: string,
		database: string,
		readOnly: boolean
	) =>
		invoke<void>('assign_user_to_database_cmd', {
			serverId,
			username,
			host,
			database,
			readOnly
		}),
	revokeUserFromDatabase: (serverId: string, username: string, host: string, database: string) =>
		invoke<void>('revoke_user_from_database_cmd', { serverId, username, host, database }),
	exportDatabase: (serverId: string, database: string) =>
		invoke<[string, number] | null>('export_database_cmd', { serverId, database }),
	importDatabase: (serverId: string, database: string) =>
		invoke<string | null>('import_database_cmd', { serverId, database }),
	exportDbServer: (serverId: string) =>
		invoke<[string, number, number] | null>('export_db_server_cmd', { serverId }),
	importDbServer: (serverId: string) =>
		invoke<string | null>('import_db_server_cmd', { serverId }),
	onDbTransferProgress: (cb: (payload: DbTransferProgressEvent) => void): Promise<UnlistenFn> =>
		listen<DbTransferProgressEvent>('db-transfer-progress', (e) => cb(e.payload)),

	// GitHub Releases update checker
	checkForUpdates: () => invoke<UpdateCheckResult>('check_for_updates_cmd'),

	// Live host system telemetry (CPU, RAM, Network)
	getSystemStats: () => invoke<SystemStats>('get_system_stats_cmd')
};
