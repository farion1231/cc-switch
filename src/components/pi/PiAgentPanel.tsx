import { forwardRef, useEffect, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save } from "lucide-react";
import { piApi } from "@/lib/api";
import type {
	PiProviderDraft,
	PiProvidersMap,
	PiModelDraft,
	PiHeaderDraft,
} from "@/types/pi";
import {
	emptyPiProviderDraft,
	PiProviderForm,
} from "@/components/pi/PiProviderForm";
import { PiProviderList } from "@/components/pi/PiProviderList";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";

export interface PiAgentPanelHandle {
	openAdd: () => void;
}

export const PiAgentPanel = forwardRef<PiAgentPanelHandle>((_props, ref) => {
	const { t } = useTranslation();
	const [providers, setProviders] = useState<PiProvidersMap>({});
	const [draft, setDraft] = useState<PiProviderDraft>(emptyPiProviderDraft);
	const [view, setView] = useState<"list" | "edit">("list");
	const [isSaving, setIsSaving] = useState(false);
	const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

	const refresh = async () => {
		try {
			setProviders(await piApi.listProviders());
		} catch (error) {
			toast.error(t("pi.toast.readFailed"), {
				description: String(error),
			});
		}
	};

	useEffect(() => {
		void refresh();
	}, []);

	const startNew = () => {
		setDraft({ ...emptyPiProviderDraft });
		setView("edit");
	};

	useImperativeHandle(ref, () => ({
		openAdd: startNew,
	}));

	const editProvider = (providerId: string) => {
		const provider = providers[providerId] as
			| Record<string, unknown>
			| undefined;
		if (!provider) {
			startNew();
			return;
		}

		// Parse models array from existing config
		const rawModels = Array.isArray(provider.models) ? provider.models : [];
		const models: PiModelDraft[] =
			rawModels.length > 0
				? rawModels.map((m: Record<string, unknown>) => ({
						id: String(m.id ?? ""),
						name: typeof m.name === "string" ? m.name : null,
						nameTouched: typeof m.name === "string",
						reasoning: Boolean(m.reasoning),
						input: Array.isArray(m.input) ? (m.input as string[]) : undefined,
						contextWindow:
							typeof m.contextWindow === "number" ? m.contextWindow : undefined,
						maxTokens:
							typeof m.maxTokens === "number" ? m.maxTokens : undefined,
					}))
				: [{ id: "", name: "", nameTouched: false }];

		// Parse headers
		const rawHeaders =
			typeof provider.headers === "object" && provider.headers !== null
				? (provider.headers as Record<string, unknown>)
				: {};
		const headers: PiHeaderDraft[] = Object.entries(rawHeaders).map(
			([key, val]) => ({
				key,
				value: String(val ?? ""),
			}),
		);

		// Parse apiKey
		const rawApiKey =
			typeof provider.apiKey === "string" ? provider.apiKey : "";
		let apiKey = emptyPiProviderDraft.apiKey;
		if (rawApiKey.startsWith("$")) {
			apiKey = { mode: "env", value: rawApiKey.slice(1) };
		} else if (rawApiKey.startsWith("!")) {
			apiKey = { mode: "command", value: rawApiKey };
		} else if (rawApiKey) {
			apiKey = { mode: "literal", value: rawApiKey };
		} else {
			apiKey = { mode: "none", value: "" };
		}

		// Parse compat
		const rawCompat =
			typeof provider.compat === "object" && provider.compat !== null
				? (provider.compat as Record<string, unknown>)
				: null;

		setDraft({
			mode: "custom",
			providerId,
			template: "custom",
			baseUrl: typeof provider.baseUrl === "string" ? provider.baseUrl : "",
			api:
				typeof provider.api === "string" ? provider.api : "openai-completions",
			apiKey,
			headers,
			models,
			compat: rawCompat
				? {
						supportsDeveloperRole: rawCompat.supportsDeveloperRole as
							| boolean
							| undefined,
						supportsReasoningEffort: rawCompat.supportsReasoningEffort as
							| boolean
							| undefined,
						supportsUsageInStreaming: rawCompat.supportsUsageInStreaming as
							| boolean
							| undefined,
						supportsEagerToolInputStreaming:
							rawCompat.supportsEagerToolInputStreaming as
								| boolean
								| undefined,
						forceAdaptiveThinking: rawCompat.forceAdaptiveThinking as
							| boolean
							| undefined,
					}
				: null,
			advancedJson: null,
		});
		setView("edit");
	};

	const saveProvider = async () => {
		if (!draft.providerId.trim()) {
			toast.error(
				t("pi.save.providerIdRequired", {
					defaultValue: "请填写供应商标识",
				}),
			);
			return;
		}

		setIsSaving(true);
		try {
			const preview = await piApi.previewProviderPatch(draft);
			const result = await piApi.applyProviderPatch(
				draft,
				preview.currentFileHash,
			);
			toast.success(t("pi.toast.saved"), {
				description: t("pi.toast.savedDesc", { path: result.backupPath }),
			});
			await refresh();
			setView("list");
		} catch (error) {
			toast.error(t("pi.toast.applyFailed"), {
				description: String(error),
			});
		} finally {
			setIsSaving(false);
		}
	};

	const confirmDeleteProvider = (providerId: string) => {
		setDeleteTarget(providerId);
	};

	const executeDelete = async () => {
		if (!deleteTarget) return;
		setIsSaving(true);
		try {
			const tempDraft: PiProviderDraft = {
				...emptyPiProviderDraft,
				providerId: deleteTarget,
			};
			const preview = await piApi.previewProviderPatch(tempDraft);
			const result = await piApi.deleteProvider(
				deleteTarget,
				preview.currentFileHash,
			);
			toast.success(t("pi.toast.deleted"), {
				description: t("pi.toast.savedDesc", { path: result.backupPath }),
			});
			setDraft({ ...emptyPiProviderDraft });
			await refresh();
		} catch (error) {
			toast.error(t("pi.toast.deleteFailed"), {
				description: String(error),
			});
		} finally {
			setIsSaving(false);
			setDeleteTarget(null);
		}
	};

	const duplicateProvider = (providerId: string) => {
		editProvider(providerId);
		// After editProvider sets the draft, override the providerId to force "new"
		setDraft((prev) => ({
			...prev,
			providerId: `${prev.providerId}-copy`,
		}));
	};

	const testConnectivity = async (providerId: string) => {
		const provider = providers[providerId] as
			| Record<string, unknown>
			| undefined;
		const baseUrl =
			typeof provider?.baseUrl === "string" ? provider.baseUrl : "";
		const normalizedUrl = baseUrl.replace(/\/+$/, "");

		try {
			const result = await piApi.testConnectivity(providerId);
			if (result.reachable) {
				toast.success(t("pi.toast.reachable", { id: providerId }), {
					description: t("pi.toast.reachableDesc", {
						url: normalizedUrl,
						status: result.statusCode ?? 0,
					}),
				});
			} else if (result.errorKind === "noBaseUrl") {
				toast.error(t("pi.toast.noBaseUrl"));
			} else if (result.errorKind === "timeout") {
				toast.error(t("pi.toast.timeout", { id: providerId }), {
					description: baseUrl,
				});
			} else {
				toast.error(t("pi.toast.unreachable", { id: providerId }), {
					description: result.detail ?? "",
				});
			}
		} catch (error) {
			toast.error(t("pi.toast.unreachable", { id: providerId }), {
				description: String(error),
			});
		}
	};

	return (
		<div className="px-6 pt-4 pb-12">
			{view === "list" ? (
				<PiProviderList
					providers={providers}
					onEdit={editProvider}
					onDuplicate={duplicateProvider}
					onDelete={confirmDeleteProvider}
					onTestConnectivity={testConnectivity}
				/>
			) : (
				<FullScreenPanel
					isOpen={view === "edit"}
					title={t("pi.editor.title", { defaultValue: "编辑供应商" })}
					onClose={() => setView("list")}
					footer={
						<Button
							type="button"
							onClick={() => void saveProvider()}
							disabled={isSaving}
							className="bg-primary text-primary-foreground hover:bg-primary/90"
						>
							<Save className="h-4 w-4 mr-2" />
							{t("common.save")}
						</Button>
					}
				>
					<div className="max-w-4xl mx-auto">
						<PiProviderForm value={draft} onChange={setDraft} />
					</div>
				</FullScreenPanel>
			)}

			<ConfirmDialog
				isOpen={deleteTarget !== null}
				title={t("pi.deleteConfirm.title", { defaultValue: "删除供应商" })}
				message={t("pi.deleteConfirm.message", {
					id: deleteTarget ?? "",
					defaultValue: `确定要删除供应商 "${deleteTarget}" 吗？此操作不可撤销。`,
				})}
				confirmText={t("common.delete")}
				variant="destructive"
				onConfirm={() => void executeDelete()}
				onCancel={() => setDeleteTarget(null)}
			/>
		</div>
	);
});

PiAgentPanel.displayName = "PiAgentPanel";
