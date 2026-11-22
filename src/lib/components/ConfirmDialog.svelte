<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		open: boolean;
		title: string;
		confirmLabel?: string;
		cancelLabel?: string;
		destructive?: boolean;
		onConfirm: () => void;
		onCancel: () => void;
		children: Snippet;
	}

	let {
		open = $bindable(false),
		title,
		confirmLabel = 'Confirm',
		cancelLabel = 'Cancel',
		destructive = false,
		onConfirm,
		onCancel,
		children
	}: Props = $props();

	let dialogEl: HTMLDialogElement | undefined = $state();
	const titleId = `dlg-${Math.random().toString(36).slice(2, 9)}`;

	$effect(() => {
		if (!dialogEl) return;
		if (open && !dialogEl.open) {
			dialogEl.showModal();
		} else if (!open && dialogEl.open) {
			dialogEl.close();
		}
	});

	function handleClose() {
		if (!dialogEl) return;
		const val = dialogEl.returnValue;
		// Reset returnValue so stale values cannot leak on subsequent Esc closes
		dialogEl.returnValue = '';
		open = false;
		if (val === 'confirm') {
			onConfirm();
		} else {
			onCancel();
		}
	}
</script>

<dialog bind:this={dialogEl} aria-labelledby={titleId} onclose={handleClose}>
	<div class="p-5">
		<h2 id={titleId} class="text-base font-semibold" style="color: var(--text-primary);">
			{title}
		</h2>
		<div class="mt-2.5 text-[13.5px] leading-relaxed" style="color: var(--text-secondary);">
			{@render children()}
		</div>
	</div>
	<form
		method="dialog"
		class="flex items-center justify-end gap-2 border-t px-5 py-3"
		style="border-color: var(--border-subtle); background-color: var(--bg-sidebar);"
	>
		<!-- Safe action gets autofocus for destructive confirmations -->
		<!-- svelte-ignore a11y_autofocus -->
		<button type="submit" value="cancel" autofocus class="wb-btn">
			{cancelLabel}
		</button>
		<button
			type="submit"
			value="confirm"
			class={destructive ? 'wb-btn-danger' : 'wb-btn-primary'}
		>
			{confirmLabel}
		</button>
	</form>
</dialog>
