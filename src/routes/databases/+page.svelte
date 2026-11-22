<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { wb } from '$lib/stores/workbench.svelte';
	import { api } from '$lib/api';
	import { formatBytes } from '$lib/format';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import DbTransferBanner from '$lib/components/DbTransferBanner.svelte';
	import TechIcon from '$lib/components/TechIcon.svelte';
	import type { DbServerInspection, DbServerProfile } from '$lib/types';
	import {
		Database,
		Users,
		User,
		UserPlus,
		Plus,
		RefreshCw,
		Download,
		Upload,
		Trash2,
		ShieldCheck,
		Lock,
		Eye,
		EyeOff,
		X,
		AlertCircle,
		Settings2,
		KeyRound,
		Search,
		ArrowUpRight,
		Table2
	} from 'lucide-svelte';

	type DbEngine = 'mariadb' | 'mysql' | 'postgresql' | 'redis';

	const ENGINE_META: Record<DbEngine, { label: string; defaultPort: number; defaultUser: string }> = {
		mariadb: { label: 'MariaDB', defaultPort: 3306, defaultUser: 'root' },
		mysql: { label: 'MySQL', defaultPort: 3306, defaultUser: 'root' },
		postgresql: { label: 'PostgreSQL', defaultPort: 5432, defaultUser: 'postgres' },
		redis: { label: 'Redis', defaultPort: 6379, defaultUser: '' }
	};

	function engineOf(srv?: DbServerProfile | null): DbEngine {
		const raw = (srv?.engine ?? 'mariadb').toLowerCase();
		if (raw === 'mysql' || raw === 'postgresql' || raw === 'redis') return raw;
		return 'mariadb';
	}

	const servers = $derived<DbServerProfile[]>(
		wb.config.db_servers && wb.config.db_servers.length > 0
			? wb.config.db_servers
			: [
					{
						id: 'local-mariadb',
						name: 'Local MariaDB (3306)',
						host: '127.0.0.1',
						port: 3306,
						username: 'root',
						password: ''
					}
				]
	);

	let activeServerId = $state('local-mariadb');
	let activeTab = $state<'databases' | 'users'>('databases');
	let inspection = $state<DbServerInspection | null>(null);
	let loading = $state(false);
	let query = $state('');
	let showSystemDbs = $state(false);
	let showCreateDb = $state(false);

	// Server form (Add / Edit)
	let serverFormMode = $state<'closed' | 'add' | 'edit'>('closed');
	let srvId = $state('');
	let srvName = $state('');
	let srvEngine = $state<DbEngine>('mariadb');
	let srvHost = $state('127.0.0.1');
	let srvPort = $state(3306);
	let srvUser = $state('root');
	let srvPass = $state('');

	// Quick inline password retry on auth failure
	let quickUser = $state('root');
	let quickPass = $state('');

	// Create Database form
	let newDbName = $state('');
	let newDbCharset = $state('utf8mb4');
	let newDbCollation = $state('utf8mb4_unicode_ci');

	// Create User form
	let userQuery = $state('');
	let showSystemUsers = $state(true);
	let showCreateUser = $state(false);
	let newUsername = $state('');
	let newUserHost = $state('localhost');
	let newUserPass = $state('');
	let showNewUserPass = $state(false);
	let newUserInitialDb = $state('');
	let newUserReadOnly = $state(false);

	// Per-card inline grant drawer
	let grantingUserKey = $state<string | null>(null);
	let assignDbName = $state('');
	let assignReadOnly = $state(false);

	// Destructive confirm dialog
	let confirmOpen = $state(false);
	let confirmTitle = $state('');
	let confirmBody = $state('');
	let confirmAction = $state<(() => Promise<void>) | null>(null);

	let busyAction = $state(false);

	const activeServer = $derived(servers.find((s) => s.id === activeServerId) ?? servers[0]);
	const userDatabases = $derived((inspection?.databases ?? []).filter((d) => !d.is_system));
	const filteredDatabases = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return (inspection?.databases ?? []).filter((d) => {
			if (!showSystemDbs && d.is_system) return false;
			if (q && !d.name.toLowerCase().includes(q)) return false;
			return true;
		});
	});
	const filteredUsers = $derived.by(() => {
		const q = userQuery.trim().toLowerCase();
		return (inspection?.users ?? [])
			.filter((u) => {
				if (!showSystemUsers && u.is_protected) return false;
				if (!q) return true;
				return (
					u.username.toLowerCase().includes(q) ||
					u.host.toLowerCase().includes(q) ||
					u.grants.some((g) => g.database.toLowerCase().includes(q))
				);
			})
			.sort((a, b) => {
				if (a.is_protected !== b.is_protected) return a.is_protected ? 1 : -1;
				return a.username.localeCompare(b.username);
			});
	});

	let serverReqSeq = 0;

	async function loadServer(serverId: string) {
		const seq = ++serverReqSeq;
		activeServerId = serverId;
		inspection = null;
		loading = true;
		try {
			const res = await api.inspectDbServer(serverId);
			if (seq !== serverReqSeq) return;
			inspection = res;
			if (res?.server) {
				quickUser = res.server.username;
				quickPass = res.server.password;
			}
		} catch (e) {
			if (seq !== serverReqSeq) return;
			inspection = null;
			wb.showToast(`Failed inspecting server: ${e}`, 'error');
		} finally {
			if (seq === serverReqSeq) {
				loading = false;
			}
		}
	}

	onMount(() => {
		const initial = servers[0]?.id ?? 'local-mariadb';
		loadServer(initial);
	});

	function openDatabaseDetail(dbName: string) {
		goto(
			`/databases/detail?server=${encodeURIComponent(activeServerId)}&db=${encodeURIComponent(dbName)}`
		);
	}

	function openAddServer() {
		if (serverFormMode === 'add') {
			serverFormMode = 'closed';
			return;
		}
		serverFormMode = 'add';
		srvId = '';
		srvName = '';
		srvEngine = 'mariadb';
		srvHost = '127.0.0.1';
		srvPort = 3306;
		srvUser = 'root';
		srvPass = '';
	}

	function onEngineChange(next: DbEngine) {
		srvEngine = next;
		const meta = ENGINE_META[next];
		srvPort = meta.defaultPort;
		srvUser = meta.defaultUser;
	}

	function openEditServer(server: DbServerProfile) {
		if (serverFormMode === 'edit' && srvId === server.id) {
			serverFormMode = 'closed';
			return;
		}
		serverFormMode = 'edit';
		srvId = server.id;
		srvName = server.name;
		srvEngine = engineOf(server);
		srvHost = server.host;
		srvPort = server.port;
		srvUser = server.username;
		srvPass = server.password;
	}

	async function handleSaveServer() {
		const name = srvName.trim();
		if (!name) return;
		const id =
			serverFormMode === 'edit' && srvId
				? srvId
				: `${srvEngine}-${name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}-${srvPort}`;
		try {
			const updated = await api.saveDbServer({
				id,
				name,
				engine: srvEngine,
				host: srvHost.trim() || '127.0.0.1',
				port: Number(srvPort) || ENGINE_META[srvEngine].defaultPort,
				username: srvUser.trim(),
				password: srvPass
			});
			wb.config = { ...wb.config, db_servers: updated };
			serverFormMode = 'closed';
			wb.showToast(`Saved server: ${name}`, 'success');
			await loadServer(id);
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		}
	}

	async function handleQuickConnect() {
		if (!activeServer) return;
		busyAction = true;
		try {
			const updated = await api.saveDbServer({
				...activeServer,
				username: quickUser.trim() || 'root',
				password: quickPass
			});
			wb.config = { ...wb.config, db_servers: updated };
			await loadServer(activeServer.id);
			if (inspection?.connected) {
				wb.showToast(`Connected to ${activeServer.name}`, 'success');
			}
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}

	async function handleRemoveServer(id: string) {
		if (servers.length <= 1) return;
		try {
			const updated = await api.removeDbServer(id);
			wb.config = { ...wb.config, db_servers: updated };
			wb.showToast('Removed server profile', 'info');
			if (updated.length > 0) {
				await loadServer(updated[0].id);
			}
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		}
	}

	async function handleCreateDb() {
		const name = newDbName.trim();
		if (!name) return;
		busyAction = true;
		try {
			await api.createDatabase(activeServerId, name, newDbCharset, newDbCollation);
			newDbName = '';
			showCreateDb = false;
			wb.showToast(`Created database '${name}'`, 'success');
			await loadServer(activeServerId);
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}

	function askDropDb(name: string) {
		confirmTitle = `Drop database '${name}'?`;
		confirmBody = `This permanently deletes all tables and data inside '${name}'.`;
		confirmAction = async () => {
			try {
				await api.dropDatabase(activeServerId, name);
				wb.showToast(`Dropped database '${name}'`, 'info');
				await loadServer(activeServerId);
			} catch (e) {
				wb.showToast(`${e}`, 'error');
			}
		};
		confirmOpen = true;
	}

	async function handleExportDb(dbName: string) {
		busyAction = true;
		try {
			const res = await api.exportDatabase(activeServerId, dbName);
			if (res) {
				const [path, bytes] = res;
				wb.showToast(`Exported ${dbName} (${formatBytes(bytes)}) to ${path}`, 'success');
			}
		} catch (e) {
			wb.showToast(`Export failed: ${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}

	async function handleImportDb(dbName: string) {
		busyAction = true;
		try {
			const path = await api.importDatabase(activeServerId, dbName);
			if (path) {
				wb.showToast(`Imported SQL into ${dbName}`, 'success');
				await loadServer(activeServerId);
			}
		} catch (e) {
			wb.showToast(`Import failed: ${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}

	async function handleExportServer(serverId: string, serverName: string) {
		busyAction = true;
		try {
			const res = await api.exportDbServer(serverId);
			if (res) {
				const [path, bytes, dbCount] = res;
				wb.showToast(
					`Exported ${dbCount} databases from ${serverName} (${formatBytes(bytes)}) to ${path}`,
					'success'
				);
			}
		} catch (e) {
			wb.showToast(`Server export failed: ${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}

	async function handleImportServer(serverId: string, serverName: string) {
		busyAction = true;
		try {
			const path = await api.importDbServer(serverId);
			if (path) {
				wb.showToast(`Imported SQL dump into ${serverName}`, 'success');
				await loadServer(serverId);
			}
		} catch (e) {
			wb.showToast(`Server import failed: ${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}

	async function handleCreateUser() {
		const user = newUsername.trim();
		if (!user) return;
		const host = newUserHost.trim() || 'localhost';
		busyAction = true;
		try {
			await api.createDbUser(activeServerId, user, host, newUserPass);
			if (newUserInitialDb) {
				await api.assignUserToDatabase(
					activeServerId,
					user,
					host,
					newUserInitialDb,
					newUserReadOnly
				);
			}
			newUsername = '';
			newUserPass = '';
			newUserInitialDb = '';
			newUserReadOnly = false;
			showNewUserPass = false;
			showCreateUser = false;
			wb.showToast(`Created user '${user}@${host}'`, 'success');
			await loadServer(activeServerId);
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}

	function toggleInlineGrant(userKey: string) {
		if (grantingUserKey === userKey) {
			grantingUserKey = null;
		} else {
			grantingUserKey = userKey;
			assignDbName = '';
			assignReadOnly = false;
		}
	}

	function askDropUser(username: string, host: string) {
		confirmTitle = `Delete user '${username}'@'${host}'?`;
		confirmBody = `This revokes all access and removes the database account '${username}'@'${host}'.`;
		confirmAction = async () => {
			try {
				await api.dropDbUser(activeServerId, username, host);
				wb.showToast(`Deleted user '${username}'@'${host}'`, 'info');
				await loadServer(activeServerId);
			} catch (e) {
				wb.showToast(`${e}`, 'error');
			}
		};
		confirmOpen = true;
	}

	async function handleAssignUser(username: string, host: string) {
		if (!assignDbName) return;
		busyAction = true;
		try {
			await api.assignUserToDatabase(
				activeServerId,
				username,
				host,
				assignDbName,
				assignReadOnly
			);
			wb.showToast(`Granted access on '${assignDbName}' to ${username}@${host}`, 'success');
			grantingUserKey = null;
			assignDbName = '';
			assignReadOnly = false;
			await loadServer(activeServerId);
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}

	async function handleRevokeGrant(username: string, host: string, database: string) {
		busyAction = true;
		try {
			await api.revokeUserFromDatabase(activeServerId, username, host, database);
			wb.showToast(`Revoked ${username}@${host} from '${database}'`, 'info');
			await loadServer(activeServerId);
		} catch (e) {
			wb.showToast(`${e}`, 'error');
		} finally {
			busyAction = false;
		}
	}
</script>

<div class="mx-auto max-w-6xl space-y-6">
	<header
		class="flex flex-wrap items-end justify-between gap-3 border-b pb-4"
		style="border-color: var(--border-subtle);"
	>
		<div>
			<h1 class="text-xl font-bold tracking-[-0.025em]" style="color: var(--text-primary);">
				Databases
			</h1>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			<button
				type="button"
				disabled={loading}
				onclick={() => loadServer(activeServerId)}
				class="wb-btn"
			>
				<RefreshCw class="h-3.5 w-3.5 {loading ? 'animate-spin' : ''}" />
				Refresh
			</button>
			<button type="button" onclick={openAddServer} class="wb-btn-primary">
				<Plus class="h-3.5 w-3.5" />
				Add Server
			</button>
		</div>
	</header>

	<DbTransferBanner />

	<!-- Database Servers Card Grid -->
	<section class="space-y-2.5">
		<div class="flex items-center justify-between">
			<span class="wb-kicker">Servers ({servers.length})</span>
			{#if inspection?.connected && inspection.server_version}
				<span class="font-mono text-[11px]" style="color: var(--text-muted);">
					{inspection.server.username ? `${inspection.server.username}@` : ''}{inspection.server.host}:{inspection.server.port} · v{inspection.server_version}
				</span>
			{/if}
		</div>

		<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
			{#each servers as srv (srv.id)}
				{@const active = activeServerId === srv.id}
				{@const eng = engineOf(srv)}
				{@const isConnected = active && inspection?.connected}
				{@const isDisconnected = active && inspection && !inspection.connected}
				<div
					role="button"
					tabindex="0"
					onclick={() => loadServer(srv.id)}
					onkeydown={(e) => {
						if (e.key === 'Enter' || e.key === ' ') {
							e.preventDefault();
							loadServer(srv.id);
						}
					}}
					class="group flex items-center justify-between gap-3 rounded-[14px] p-3.5 text-left transition hover:bg-[var(--bg-surface-hover)] cursor-pointer"
					style="
						background-color: {active ? 'var(--accent-soft)' : 'var(--bg-surface)'};
						box-shadow: var(--shadow-card);
					"
				>
					<div class="flex items-center gap-3 min-w-0">
						<div
							class="flex h-10 w-10 shrink-0 items-center justify-center rounded-[10px]"
							style="background-color: var(--bg-elevated);"
						>
							<TechIcon name={eng} class="h-5 w-5" />
						</div>
						<div class="min-w-0">
							<div class="flex items-center gap-2 min-w-0">
								<span class="truncate text-sm font-semibold" style="color: var(--text-primary);">
									{srv.name}
								</span>
								<span
									class="shrink-0 rounded-full px-2 py-0.5 font-mono text-[10px] font-medium"
									style="background-color: var(--bg-elevated); color: var(--text-secondary);"
								>
									{ENGINE_META[eng].label}
								</span>
							</div>
							<div class="mt-0.5 flex items-center gap-1.5 font-mono text-[11px]" style="color: var(--text-muted);">
								<span
									class="h-2 w-2 shrink-0 rounded-full"
									style="background-color: {isConnected
										? 'var(--success)'
										: isDisconnected
											? 'var(--danger)'
											: 'var(--text-muted)'};"
								></span>
								<span class="truncate">{srv.host}:{srv.port}</span>
							</div>
						</div>
					</div>

					<div class="flex shrink-0 items-center gap-0.5">
						<button
							type="button"
							title="Import SQL dump into {srv.name}"
							disabled={busyAction}
							onclick={(e) => {
								e.stopPropagation();
								handleImportServer(srv.id, srv.name);
							}}
							class="rounded-full p-1.5 transition hover:bg-[var(--bg-elevated)] disabled:opacity-30"
							style="color: var(--text-muted);"
						>
							<Upload class="h-3.5 w-3.5" />
						</button>
						<button
							type="button"
							title="Export all databases from {srv.name}"
							disabled={busyAction}
							onclick={(e) => {
								e.stopPropagation();
								handleExportServer(srv.id, srv.name);
							}}
							class="rounded-full p-1.5 transition hover:bg-[var(--bg-elevated)] disabled:opacity-30"
							style="color: var(--text-muted);"
						>
							<Download class="h-3.5 w-3.5" />
						</button>
						<button
							type="button"
							title="Edit server connection"
							onclick={(e) => {
								e.stopPropagation();
								openEditServer(srv);
							}}
							class="rounded-full p-1.5 transition hover:bg-[var(--bg-elevated)]"
							style="color: var(--text-muted);"
						>
							<Settings2 class="h-3.5 w-3.5" />
						</button>
						{#if servers.length > 1}
							<button
								type="button"
								title="Remove server"
								onclick={(e) => {
									e.stopPropagation();
									handleRemoveServer(srv.id);
								}}
								class="rounded-full p-1.5 transition hover:bg-[var(--danger-soft)] hover:text-[var(--danger)]"
								style="color: var(--text-muted);"
							>
								<Trash2 class="h-3.5 w-3.5" />
							</button>
						{/if}
					</div>
				</div>
			{/each}
		</div>
	</section>

	<!-- Add / Edit Server Drawer Card -->
	{#if serverFormMode !== 'closed'}
		<section
			class="border p-5 space-y-4"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			<div class="flex items-center justify-between">
				<div class="wb-kicker">
					{serverFormMode === 'edit' ? 'Edit Server Connection' : 'New Database Server'}
				</div>
				<button
					type="button"
					onclick={() => (serverFormMode = 'closed')}
					class="rounded-full p-1 transition hover:bg-[var(--bg-surface-hover)]"
					style="color: var(--text-muted);"
				>
					<X class="h-4 w-4" />
				</button>
			</div>

			<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-6">
				<label class="space-y-1">
					<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Engine</span>
					<select
						value={srvEngine}
						onchange={(e) => onEngineChange((e.currentTarget as HTMLSelectElement).value as DbEngine)}
						class="w-full px-3 py-2 text-xs"
						style="color: var(--text-primary);"
					>
						<option value="mariadb">MariaDB</option>
						<option value="mysql">MySQL</option>
						<option value="postgresql">PostgreSQL</option>
						<option value="redis">Redis</option>
					</select>
				</label>
				<label class="space-y-1">
					<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Name</span>
					<input type="text" bind:value={srvName} placeholder="Local {ENGINE_META[srvEngine].label}" class="w-full px-3 py-2 text-xs" style="color: var(--text-primary);" />
				</label>
				<label class="space-y-1">
					<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Host</span>
					<input type="text" bind:value={srvHost} placeholder="127.0.0.1" class="w-full px-3 py-2 font-mono text-xs" style="color: var(--text-primary);" />
				</label>
				<label class="space-y-1">
					<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Port</span>
					<input type="number" bind:value={srvPort} class="w-full px-3 py-2 font-mono text-xs" style="color: var(--text-primary);" />
				</label>
				<label class="space-y-1">
					<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Username</span>
					<input type="text" bind:value={srvUser} placeholder={ENGINE_META[srvEngine].defaultUser || '(none)'} class="w-full px-3 py-2 font-mono text-xs" style="color: var(--text-primary);" />
				</label>
				<label class="space-y-1">
					<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Password</span>
					<input type="password" bind:value={srvPass} placeholder="(optional)" class="w-full px-3 py-2 font-mono text-xs" style="color: var(--text-primary);" />
				</label>
			</div>

			<div class="flex justify-end gap-2">
				<button type="button" onclick={() => (serverFormMode = 'closed')} class="wb-btn">Cancel</button>
				<button type="button" onclick={handleSaveServer} class="wb-btn-primary">
					{serverFormMode === 'edit' ? 'Save Changes' : 'Connect & Save'}
				</button>
			</div>
		</section>
	{/if}

	<!-- Connection Error / Auth Prompt -->
	{#if inspection && !inspection.connected}
		<section
			class="border p-6 space-y-4"
			style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
		>
			<div class="flex items-start gap-3">
				<AlertCircle class="mt-0.5 h-5 w-5 shrink-0" style="color: var(--danger);" />
				<div class="min-w-0 space-y-1">
					<div class="text-sm font-semibold" style="color: var(--text-primary);">
						Cannot connect to {activeServer?.name}
					</div>
					<div class="selectable font-mono text-xs" style="color: var(--text-secondary);">
						{inspection.error ?? 'Connection refused. Make sure MariaDB/MySQL is running.'}
					</div>
				</div>
			</div>

			<div class="flex flex-wrap items-end gap-2.5">
				<label class="space-y-1">
					<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">User</span>
					<input type="text" bind:value={quickUser} class="w-40 px-3 py-2 font-mono text-xs" style="color: var(--text-primary);" />
				</label>
				<label class="space-y-1">
					<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Password</span>
					<input
						type="password"
						bind:value={quickPass}
						onkeydown={(e) => e.key === 'Enter' && handleQuickConnect()}
						placeholder="Enter password"
						class="w-52 px-3 py-2 font-mono text-xs"
						style="color: var(--text-primary);"
					/>
				</label>
				<button type="button" disabled={busyAction} onclick={handleQuickConnect} class="wb-btn-primary">
					<KeyRound class="h-3.5 w-3.5" />
					Save & Reconnect
				</button>
			</div>
		</section>
	{:else if loading && !inspection}
		<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
			{#each Array(6) as _}
				<div
					class="h-28 animate-pulse border p-4"
					style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
				></div>
			{/each}
		</div>
	{:else if inspection?.connected}
		<!-- Search & View Controls Bar -->
		<div class="flex flex-wrap items-center gap-2.5">
			{#if activeTab === 'databases'}
				<div class="relative min-w-56 flex-1">
					<Search
						class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
						style="color: var(--text-muted);"
					/>
					<input
						type="search"
						bind:value={query}
						placeholder="Search {filteredDatabases.length} databases..."
						class="w-full py-2 pl-9 pr-3 text-xs outline-none"
						style="color: var(--text-primary);"
					/>
				</div>
			{:else}
				<div class="relative min-w-56 flex-1">
					<Search
						class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
						style="color: var(--text-muted);"
					/>
					<input
						type="search"
						bind:value={userQuery}
						placeholder="Search {filteredUsers.length} users or grants..."
						class="w-full py-2 pl-9 pr-3 text-xs outline-none"
						style="color: var(--text-primary);"
					/>
				</div>
			{/if}

			<div class="flex items-center gap-1.5">
				<button
					type="button"
					onclick={() => (activeTab = 'databases')}
					class="inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
					style="
						background-color: {activeTab === 'databases' ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
						color: {activeTab === 'databases' ? 'var(--accent)' : 'var(--text-secondary)'};
					"
				>
					<Database class="h-3.5 w-3.5" />
					Databases ({userDatabases.length})
				</button>
				<button
					type="button"
					onclick={() => (activeTab = 'users')}
					class="inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
					style="
						background-color: {activeTab === 'users' ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
						color: {activeTab === 'users' ? 'var(--accent)' : 'var(--text-secondary)'};
					"
				>
					<Users class="h-3.5 w-3.5" />
					Users ({inspection.users.length})
				</button>
				{#if activeTab === 'databases'}
					<button
						type="button"
						onclick={() => (showSystemDbs = !showSystemDbs)}
						class="rounded-full px-3 py-1.5 text-xs font-medium transition"
						style="
							background-color: {showSystemDbs ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
							color: {showSystemDbs ? 'var(--accent)' : 'var(--text-muted)'};
						"
					>
						System
					</button>
					<button
						type="button"
						onclick={() => (showCreateDb = !showCreateDb)}
						class="wb-btn-primary"
					>
						<Plus class="h-3.5 w-3.5" />
						New Database
					</button>
				{:else}
					<button
						type="button"
						onclick={() => (showSystemUsers = !showSystemUsers)}
						class="rounded-full px-3 py-1.5 text-xs font-medium transition"
						style="
							background-color: {showSystemUsers ? 'var(--accent-soft)' : 'var(--bg-elevated)'};
							color: {showSystemUsers ? 'var(--accent)' : 'var(--text-muted)'};
						"
					>
						System
					</button>
					<button
						type="button"
						onclick={() => (showCreateUser = !showCreateUser)}
						class="wb-btn-primary"
					>
						<UserPlus class="h-3.5 w-3.5" />
						New User
					</button>
				{/if}
			</div>
		</div>

		{#if activeTab === 'databases'}
			<!-- Inline Create Database Panel -->
			{#if showCreateDb}
				<section
					class="border p-4 space-y-3"
					style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
				>
					<div class="wb-kicker">Create Database</div>
					<div class="flex flex-wrap items-end gap-2.5">
						<label class="min-w-48 flex-1 space-y-1">
							<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Database Name</span>
							<input
								type="text"
								bind:value={newDbName}
								onkeydown={(e) => e.key === 'Enter' && handleCreateDb()}
								placeholder="laravel_app"
								class="w-full px-3 py-2 font-mono text-xs"
								style="color: var(--text-primary);"
							/>
						</label>
						<label class="w-36 space-y-1">
							<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Charset</span>
							<select bind:value={newDbCharset} class="w-full px-3 py-2 font-mono text-xs" style="color: var(--text-primary);">
								<option value="utf8mb4">utf8mb4</option>
								<option value="utf8">utf8</option>
								<option value="latin1">latin1</option>
							</select>
						</label>
						<label class="w-48 space-y-1">
							<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Collation</span>
							<select bind:value={newDbCollation} class="w-full px-3 py-2 font-mono text-xs" style="color: var(--text-primary);">
								<option value="utf8mb4_unicode_ci">utf8mb4_unicode_ci</option>
								<option value="utf8mb4_general_ci">utf8mb4_general_ci</option>
								<option value="utf8mb4_0900_ai_ci">utf8mb4_0900_ai_ci</option>
							</select>
						</label>
						<button type="button" disabled={busyAction || !newDbName.trim()} onclick={handleCreateDb} class="wb-btn-primary">
							Create
						</button>
					</div>
				</section>
			{/if}

			<!-- 3-Column Database Card Grid (mirrors Projects page) -->
			{#if filteredDatabases.length === 0}
				<div
					class="border p-10 text-center font-mono text-xs"
					style="background-color: var(--bg-surface); border-color: var(--border-subtle); color: var(--text-muted);"
				>
					NO DATABASES FOUND
				</div>
			{:else}
				<div class="wb-grid-cards">
					{#each filteredDatabases as db (db.name)}
						<div
							role="button"
							tabindex="0"
							onclick={() => openDatabaseDetail(db.name)}
							onkeydown={(e) => {
								if (e.key === 'Enter' || e.key === ' ') {
									e.preventDefault();
									openDatabaseDetail(db.name);
								}
							}}
							class="group flex flex-col justify-between border p-4 text-left transition hover:border-[var(--border-strong)] hover:bg-[var(--bg-surface-hover)] cursor-pointer"
							style="background-color: var(--bg-surface); border-color: var(--border-subtle);"
						>
							<div class="space-y-3">
								<div class="flex items-start justify-between gap-2">
									<div class="flex items-center gap-2.5 min-w-0">
										<div
											class="flex h-8 w-8 shrink-0 items-center justify-center rounded-[10px]"
											style="background-color: var(--accent-soft); color: var(--accent);"
										>
											<Database class="h-4 w-4" />
										</div>
										<div class="min-w-0">
											<div class="truncate font-mono text-sm font-semibold" style="color: var(--text-primary);">
												{db.name}
											</div>
											<div class="font-mono text-[11px]" style="color: var(--text-muted);">
												{db.charset} · {db.collation}
											</div>
										</div>
									</div>
									{#if db.is_system}
										<span
											class="shrink-0 rounded-full px-2 py-0.5 font-mono text-[10px]"
											style="background-color: var(--bg-elevated); color: var(--text-muted);"
										>
											System
										</span>
									{:else}
										<ArrowUpRight
											class="h-4 w-4 shrink-0 opacity-40 transition group-hover:opacity-100"
											style="color: var(--accent);"
										/>
									{/if}
								</div>

								<div class="flex items-center gap-3 font-mono text-xs" style="color: var(--text-secondary);">
									<span class="inline-flex items-center gap-1">
										<Table2 class="h-3.5 w-3.5" style="color: var(--text-muted);" />
										{db.table_count} {db.table_count === 1 ? 'table' : 'tables'}
									</span>
									<span style="color: var(--text-muted);">·</span>
									<span>{formatBytes(db.size_bytes)}</span>
								</div>
							</div>

							<div class="mt-3 flex items-center justify-between pt-1">
								<span class="font-mono text-[11px]" style="color: var(--accent);">
									View Tables
								</span>
								<div class="flex items-center gap-1">
									<button
										type="button"
										title="Import SQL"
										disabled={busyAction || db.is_system}
										onclick={(e) => {
											e.stopPropagation();
											handleImportDb(db.name);
										}}
										class="rounded-full p-1.5 transition hover:bg-[var(--bg-elevated)] disabled:opacity-30"
										style="color: var(--text-secondary);"
									>
										<Upload class="h-3.5 w-3.5" />
									</button>
									<button
										type="button"
										title="Export SQL"
										disabled={busyAction}
										onclick={(e) => {
											e.stopPropagation();
											handleExportDb(db.name);
										}}
										class="rounded-full p-1.5 transition hover:bg-[var(--bg-elevated)] disabled:opacity-30"
										style="color: var(--text-secondary);"
									>
										<Download class="h-3.5 w-3.5" />
									</button>
									{#if !db.is_system}
										<button
											type="button"
											title="Drop database"
											disabled={busyAction}
											onclick={(e) => {
												e.stopPropagation();
												askDropDb(db.name);
											}}
											class="rounded-full p-1.5 transition hover:bg-[var(--danger-soft)]"
											style="color: var(--danger);"
										>
											<Trash2 class="h-3.5 w-3.5" />
										</button>
									{/if}
								</div>
							</div>
						</div>
					{/each}
				</div>
			{/if}
		{:else}
			<!-- Users & Grants Tab -->
			{#if showCreateUser}
				<section
					class="rounded-2xl p-4 space-y-3.5"
					style="background-color: var(--bg-surface);"
				>
					<div class="flex items-center justify-between">
						<span class="wb-kicker">Create Database User</span>
						<button
							type="button"
							onclick={() => (showCreateUser = false)}
							class="rounded-full p-1 transition hover:bg-[var(--bg-elevated)]"
							style="color: var(--text-muted);"
							aria-label="Close create user form"
						>
							<X class="h-3.5 w-3.5" />
						</button>
					</div>

					<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
						<label class="space-y-1">
							<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Username</span>
							<input
								type="text"
								bind:value={newUsername}
								onkeydown={(e) => e.key === 'Enter' && handleCreateUser()}
								placeholder="app_user"
								class="w-full px-3 py-2 font-mono text-xs"
								style="color: var(--text-primary);"
							/>
						</label>

						<label class="space-y-1">
							<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Host</span>
							<div class="flex items-center gap-1.5">
								<input
									type="text"
									bind:value={newUserHost}
									placeholder="localhost"
									class="min-w-0 flex-1 px-3 py-2 font-mono text-xs"
									style="color: var(--text-primary);"
								/>
								<button
									type="button"
									onclick={() => (newUserHost = newUserHost === '%' ? 'localhost' : '%')}
									class="shrink-0 rounded-full px-2.5 py-2 font-mono text-[11px] transition"
									style="background-color: {newUserHost === '%' ? 'var(--accent-soft)' : 'var(--bg-elevated)'}; color: {newUserHost === '%' ? 'var(--accent)' : 'var(--text-secondary)'};"
									title="Toggle wildcard host (%)"
								>
									%
								</button>
							</div>
						</label>

						<label class="space-y-1">
							<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Password</span>
							<div class="relative">
								<input
									type={showNewUserPass ? 'text' : 'password'}
									bind:value={newUserPass}
									onkeydown={(e) => e.key === 'Enter' && handleCreateUser()}
									placeholder="Enter password"
									class="w-full py-2 pl-3 pr-8 font-mono text-xs"
									style="color: var(--text-primary);"
								/>
								<button
									type="button"
									onclick={() => (showNewUserPass = !showNewUserPass)}
									class="absolute right-2 top-1/2 -translate-y-1/2 p-1"
									style="color: var(--text-muted);"
									aria-label="Toggle password visibility"
								>
									{#if showNewUserPass}
										<EyeOff class="h-3.5 w-3.5" />
									{:else}
										<Eye class="h-3.5 w-3.5" />
									{/if}
								</button>
							</div>
						</label>

						<label class="space-y-1">
							<span class="block font-mono text-[10.5px] uppercase" style="color: var(--text-secondary);">Initial Database (Optional)</span>
							<select
								bind:value={newUserInitialDb}
								class="w-full px-3 py-2 font-mono text-xs"
								style="color: var(--text-primary);"
							>
								<option value="">None (grant later)</option>
								{#each userDatabases as d (d.name)}
									<option value={d.name}>{d.name}</option>
								{/each}
							</select>
						</label>
					</div>

					<div class="flex flex-wrap items-center justify-between gap-2 pt-0.5">
						{#if newUserInitialDb}
							<label class="inline-flex items-center gap-2 text-xs cursor-pointer" style="color: var(--text-secondary);">
								<input type="checkbox" bind:checked={newUserReadOnly} class="h-3.5 w-3.5" />
								Read-only (SELECT) on <span class="font-mono">{newUserInitialDb}</span>
							</label>
						{:else}
							<span class="font-mono text-[11px]" style="color: var(--text-muted);">
								Use <code class="px-1">%</code> as host to allow connections from any IP.
							</span>
						{/if}
						<div class="flex items-center gap-2">
							<button type="button" onclick={() => (showCreateUser = false)} class="wb-btn">
								Cancel
							</button>
							<button
								type="button"
								disabled={busyAction || !newUsername.trim()}
								onclick={handleCreateUser}
								class="wb-btn-primary"
							>
								<UserPlus class="h-3.5 w-3.5" />
								Create User
							</button>
						</div>
					</div>
				</section>
			{/if}

			{#if filteredUsers.length === 0}
				<div
					class="rounded-2xl p-10 text-center font-mono text-xs"
					style="background-color: var(--bg-surface); color: var(--text-muted);"
				>
					NO USERS FOUND
				</div>
			{:else}
				<div class="wb-grid-cards">
					{#each filteredUsers as user (`${user.username}@${user.host}`)}
						{@const userKey = `${user.username}@${user.host}`}
						{@const isGranting = grantingUserKey === userKey}
						<div
							class="flex flex-col justify-between rounded-2xl p-4 space-y-3.5 transition"
							style="background-color: var(--bg-surface);"
						>
							<div class="space-y-3">
								<!-- Card Header -->
								<div class="flex items-start justify-between gap-2">
									<div class="flex min-w-0 items-center gap-2.5">
										<div
											class="flex h-8 w-8 shrink-0 items-center justify-center rounded-[10px]"
											style="background-color: {user.is_protected
												? 'var(--bg-elevated)'
												: 'var(--accent-soft)'}; color: {user.is_protected
												? 'var(--text-muted)'
												: 'var(--accent)'};"
										>
											{#if user.is_protected}
												<Lock class="h-3.5 w-3.5" />
											{:else}
												<User class="h-4 w-4" />
											{/if}
										</div>
										<div class="min-w-0">
											<div class="flex items-center gap-1.5">
												<span
													class="truncate font-mono text-sm font-semibold"
													style="color: var(--text-primary);"
												>
													{user.username}
												</span>
											</div>
											<div class="font-mono text-[11px]" style="color: var(--text-muted);">
												{user.host ? `@${user.host}` : 'Built-in role'}
											</div>
										</div>
									</div>

									<div class="flex shrink-0 items-center gap-1">
										{#if user.is_protected}
											<span
												class="rounded-full px-2 py-0.5 font-mono text-[10px]"
												style="background-color: var(--bg-elevated); color: var(--text-muted);"
											>
												{user.username === 'root' ? 'Superuser' : 'System'}
											</span>
										{:else}
											<button
												type="button"
												title="Grant database access"
												onclick={() => toggleInlineGrant(userKey)}
												class="inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-medium transition"
												style="background-color: {isGranting
													? 'var(--accent-soft)'
													: 'var(--bg-elevated)'}; color: {isGranting
													? 'var(--accent)'
													: 'var(--text-secondary)'};"
											>
												<Plus class="h-3 w-3" />
												Grant
											</button>
											<button
												type="button"
												title="Delete user"
												onclick={() => askDropUser(user.username, user.host)}
												class="rounded-full p-1.5 transition hover:bg-[var(--danger-soft)]"
												style="color: var(--danger);"
											>
												<Trash2 class="h-3.5 w-3.5" />
											</button>
										{/if}
									</div>
								</div>

								<!-- Grants List -->
								<div class="space-y-1.5">
									<div class="flex flex-wrap gap-1.5">
										{#if user.grants.length > 0}
											{#each user.grants as g (`${g.database}-${g.privilege}`)}
												<span
													class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 font-mono text-[11px]"
													style="background-color: var(--bg-elevated); color: var(--text-primary);"
												>
													<Database class="h-3 w-3 shrink-0" style="color: var(--accent);" />
													<span class="truncate max-w-36">{g.database}</span>
													<span
														class="rounded-full px-1.5 py-0.2 text-[9.5px] font-semibold"
														style="background-color: {g.privilege === 'READ ONLY'
															? 'color-mix(in srgb, var(--text-muted) 18%, transparent)'
															: 'var(--accent-soft)'}; color: {g.privilege === 'READ ONLY'
															? 'var(--text-secondary)'
															: 'var(--accent)'};"
													>
														{g.privilege === 'READ ONLY' ? 'RO' : 'ALL'}
													</span>
													{#if !user.is_protected}
														<button
															type="button"
															title="Revoke access to {g.database}"
															onclick={() => handleRevokeGrant(user.username, user.host, g.database)}
															class="rounded-full p-0.5 transition hover:text-[var(--danger)]"
															style="color: var(--text-muted);"
														>
															<X class="h-3 w-3" />
														</button>
													{/if}
												</span>
											{/each}
										{:else if user.username === 'root'}
											<span
												class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 font-mono text-[11px]"
												style="background-color: var(--bg-elevated); color: var(--text-secondary);"
											>
												<ShieldCheck class="h-3 w-3" style="color: var(--success);" />
												Global privileges (*.*)
											</span>
										{:else if user.is_protected}
											<span class="font-mono text-[11px]" style="color: var(--text-muted);">
												Internal system account
											</span>
										{:else}
											<span class="font-mono text-[11px]" style="color: var(--text-muted);">
												No database access yet
											</span>
										{/if}
									</div>
								</div>
							</div>

							<!-- Inline Per-Card Grant Drawer -->
							{#if isGranting && !user.is_protected}
								<div
									class="space-y-2.5 rounded-xl p-3"
									style="background-color: var(--bg-app);"
								>
									<select
										bind:value={assignDbName}
										class="w-full px-2.5 py-1.5 font-mono text-xs"
										style="color: var(--text-primary);"
									>
										<option value="">Select database...</option>
										{#each userDatabases as d (d.name)}
											<option value={d.name}>{d.name}</option>
										{/each}
									</select>
									<div class="flex items-center justify-between gap-2">
										<label
											class="inline-flex items-center gap-1.5 text-[11px] cursor-pointer"
											style="color: var(--text-secondary);"
										>
											<input type="checkbox" bind:checked={assignReadOnly} class="h-3.5 w-3.5" />
											Read-only
										</label>
										<div class="flex items-center gap-1.5">
											<button
												type="button"
												onclick={() => (grantingUserKey = null)}
												class="rounded-full px-2.5 py-1 text-[11px] transition hover:bg-[var(--bg-surface)]"
												style="color: var(--text-muted);"
											>
												Cancel
											</button>
											<button
												type="button"
												disabled={busyAction || !assignDbName}
												onclick={() => handleAssignUser(user.username, user.host)}
												class="wb-btn-primary !px-3 !py-1 !text-[11px]"
											>
												<ShieldCheck class="h-3 w-3" />
												Grant
											</button>
										</div>
									</div>
								</div>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		{/if}
	{/if}
</div>

<ConfirmDialog
	bind:open={confirmOpen}
	title={confirmTitle}
	confirmLabel="Delete"
	destructive={true}
	onConfirm={() => {
		confirmAction?.();
	}}
	onCancel={() => {}}
>
	<p>{confirmBody}</p>
</ConfirmDialog>
