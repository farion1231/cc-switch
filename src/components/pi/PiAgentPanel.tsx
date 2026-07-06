import { forwardRef, useEffect, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { piApi } from "@/lib/api";
import type {
	PiProviderDraft,
	PiProviderPatchPreview,
	PiProvidersMap,
	PiModelDraft,
	PiHeaderDraft,
} from "@/types/pi";
import {
	emptyPiProviderDraft,
	PiProviderForm,
} from "@/components/pi/PiProviderForm";
import { PiProviderList } from "@/components/pi/PiProviderList";
import { PiProviderDiffPreview } from "@/components/pi/PiProviderDiffPreview";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export interface PiAgentPanelHandle {
	openAdd: () => void;
}

export const PiAgentPanel = forwardRef<PiAgentPanelHandle>((_props, ref) => {
	const { t } = useTranslation();
	const [providers, setProviders] = useState<PiProvidersMap>({});
	const [draft, setDraft] = useState<PiProviderDraft>(emptyPiProviderDraft);
	const [preview, setPreview] = useState<PiProviderPatchPreview | null>(null);
	const [activeTab, setActiveTab] = useState("providers");
	const [isApplying, setIsApplying] = useState(false);

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
		setPreview(null);
		setActiveTab("edit");
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
							rawCompat.supportsEagerToolInputStreaming as boolean | undefined,
						forceAdaptiveThinking: rawCompat.forceAdaptiveThinking as
							| boolean
							| undefined,
					}
				: null,
			advancedJson: null,
		});
		setPreview(null);
		setActiveTab("edit");
	};

	const buildPreview = async () => {
		try {
			const next = await piApi.previewProviderPatch(draft);
			setPreview(next);
			setActiveTab("review");
		} catch (error) {
			toast.error(t("pi.toast.previewFailed"), {
				description: String(error),
			});
		}
	};

	const applyPreview = async () => {
		if (!preview) return;
		setIsApplying(true);
		try {
			const result = await piApi.applyProviderPatch(
				draft,
				preview.currentFileHash,
			);
			toast.success(t("pi.toast.saved"), {
				description: t("pi.toast.savedDesc", { path: result.backupPath }),
			});
			setPreview(null);
			await refresh();
			setActiveTab("providers");
		} catch (error) {
			toast.error(t("pi.toast.applyFailed"), {
				description: String(error),
			});
		} finally {
			setIsApplying(false);
		}
	};

	const deletePreview = async () => {
		if (!preview || !draft.providerId.trim()) return;
		setIsApplying(true);
		try {
			const result = await piApi.deleteProvider(
				draft.providerId,
				preview.currentFileHash,
			);
			toast.success(t("pi.toast.deleted"), {
				description: t("pi.toast.savedDesc", { path: result.backupPath }),
			});
			setDraft({ ...emptyPiProviderDraft });
			setPreview(null);
			await refresh();
			setActiveTab("providers");
		} catch (error) {
			toast.error(t("pi.toast.deleteFailed"), {
				description: String(error),
			});
		} finally {
			setIsApplying(false);
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

	const deleteProviderDirect = async (providerId: string) => {
		try {
			// First get a preview to obtain the current file hash
			const tempDraft: PiProviderDraft = {
				...emptyPiProviderDraft,
				providerId,
			};
			const previewData = await piApi.previewProviderPatch(tempDraft);
			const result = await piApi.deleteProvider(
				providerId,
				previewData.currentFileHash,
			);
			toast.success(t("pi.toast.deleted"), {
				description: t("pi.toast.savedDesc", { path: result.backupPath }),
			});
			await refresh();
		} catch (error) {
			toast.error(t("pi.toast.deleteFailed"), {
				description: String(error),
			});
		}
	};

	return (
		<div className="px-6 pt-4 pb-12">
			<Card>
				<CardContent className="pt-6">
					<Tabs value={activeTab} onValueChange={setActiveTab}>
						<TabsList>
							<TabsTrigger value="providers">
								{t("pi.tabs.providers")}
								{Object.keys(providers).length > 0 && (
									<span className="ml-1.5 text-xs text-muted-foreground">
										({Object.keys(providers).length})
									</span>
								)}
							</TabsTrigger>
							<TabsTrigger value="edit">{t("pi.tabs.edit")}</TabsTrigger>
							<TabsTrigger value="review" disabled={!preview}>
								{t("pi.tabs.review")}
							</TabsTrigger>
						</TabsList>
						<TabsContent value="providers" className="pt-4">
							<PiProviderList
								providers={providers}
								onEdit={editProvider}
								onDuplicate={duplicateProvider}
								onDelete={deleteProviderDirect}
								onTestConnectivity={testConnectivity}
							/>
						</TabsContent>
						<TabsContent value="edit" className="pt-4">
							<div className="space-y-4">
								<PiProviderForm value={draft} onChange={setDraft} />
								<div className="flex gap-2 pt-2 border-t">
									<Button
										type="button"
										onClick={() => void buildPreview()}
										disabled={!draft.providerId.trim()}
									>
										{t("pi.previewReview")}
									</Button>
									<span className="text-xs text-muted-foreground self-center">
										{t("pi.previewHint")}
									</span>
								</div>
							</div>
						</TabsContent>
						<TabsContent value="review" className="pt-4">
							<PiProviderDiffPreview
								preview={preview}
								isApplying={isApplying}
								onApply={() => void applyPreview()}
								onDelete={() => void deletePreview()}
								canDelete={Boolean(draft.providerId.trim())}
							/>
						</TabsContent>
					</Tabs>
				</CardContent>
			</Card>
		</div>
	);
});

PiAgentPanel.displayName = "PiAgentPanel";
