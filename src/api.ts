import { invoke } from "@tauri-apps/api/core";

export type Trigger = {
  id: number;
  name: string;
  enabled: boolean;
  action_type: string;
  action_config: string;
  public_id: string;
  tag_written: boolean;
  verified_at: string | null;
  created_at: string;
};

export type TagPayload = {
  trigger_id: number;
  trigger_name: string;
  uri: string;
  write_page: string;
  public_id: string;
  bytes: number;
  tag_written: boolean;
};

export type Run = {
  id: number;
  trigger_id: number;
  trigger_name: string;
  status: string;
  source: string;
  started_at: string;
  finished_at: string | null;
  error: string | null;
  external_run_id: string | null;
};

export type Settings = {
  device_name: string;
  port: number;
  auto_start: boolean;
  bridge_url: string;
  bridge_token: string;
  integration_mode: "follow" | "independent" | "manual" | null;
};

export type BridgeHealth = {
  app: string;
  version: string;
  bridgeApiVersion: number;
};

export type BridgeItem = {
  id: number;
  name: string;
  enabled: boolean;
};

export type ImportedBridge = {
  app: string;
  url: string;
  token: string;
  version: string | null;
};

export type AnniversaryProject = {
  id: number;
  slug: string;
  name: string;
  event_date: string;
  title: string;
  recipient: string;
  opening: string;
  body: string;
  closing: string;
  template_id: string;
  theme: string;
  photos: string[];
  custom_css: string;
  created_at: string;
  updated_at: string;
};

export type ProjectInput = Omit<
  AnniversaryProject,
  "id" | "slug" | "photos" | "created_at" | "updated_at"
>;

export type ProjectVersion = {
  id: number;
  project_id: number;
  version: number;
  published_url: string;
  status: string;
  published_at: string;
};

export type AiProvider = {
  id: number;
  name: string;
  kind: string;
  base_url: string;
  model: string;
  api_key: string;
  is_default: boolean;
  created_at: string;
};

export type AiProviderInput = Omit<AiProvider, "id" | "created_at">;

export type AiGenerateInput = {
  relationship: string;
  tone: string;
  keywords: string;
  language: string;
  length: string;
};

export type GeneratedCopy = {
  title: string;
  opening: string;
  body: string;
  closing: string;
};

export type PublishTarget = {
  id: number;
  name: string;
  kind: string;
  endpoint: string;
  region: string;
  bucket: string;
  access_key: string;
  secret_key: string;
  prefix: string;
  public_base_url: string;
  cloudflared_path: string;
  tunnel_token: string;
  hostname: string;
  account_id: string;
  project_name: string;
  api_token: string;
  branch: string;
  is_default: boolean;
  created_at: string;
};

export type PublishTargetInput = Omit<PublishTarget, "id" | "created_at">;

export type PublishResult = {
  url: string;
  slug: string;
  uploaded: number;
};

export type TemplateInfo = {
  id: string;
  name: string;
};

export type Status = {
  running: boolean;
  port: number;
  ip: string;
  version: string;
  app: string;
};

export const api = {
  status: () => invoke<Status>("get_status"),
  triggers: () => invoke<Trigger[]>("list_triggers"),
  createTrigger: (name: string, actionType: string, actionConfig: string) =>
    invoke<Trigger>("create_trigger", { name, actionType, actionConfig }),
  updateTrigger: (
    id: number,
    name: string,
    enabled: boolean,
    actionType: string,
    actionConfig: string,
  ) => invoke<void>("update_trigger", { id, name, enabled, actionType, actionConfig }),
  setTriggerEnabled: (id: number, enabled: boolean) =>
    invoke<void>("set_trigger_enabled", { id, enabled }),
  deleteTrigger: (id: number) => invoke<void>("delete_trigger", { id }),
  tagPayload: (triggerId: number) =>
    invoke<TagPayload>("get_tag_payload", { triggerId }),
  markTagWritten: (id: number) => invoke<void>("mark_tag_written", { id }),
  runs: (limit = 50) => invoke<Run[]>("list_runs", { limit }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  testBridge: (bridgeUrl: string, bridgeToken: string) =>
    invoke<BridgeHealth>("test_bridge", { bridgeUrl, bridgeToken }),
  bridgeTasks: (bridgeUrl: string, bridgeToken: string) =>
    invoke<BridgeItem[]>("bridge_tasks", { bridgeUrl, bridgeToken }),
  bridgeWorkflows: (bridgeUrl: string, bridgeToken: string) =>
    invoke<BridgeItem[]>("bridge_workflows", { bridgeUrl, bridgeToken }),
  importBridge: () => invoke<ImportedBridge>("import_bridge"),
  templates: () => invoke<TemplateInfo[]>("list_templates"),
  projects: () => invoke<AnniversaryProject[]>("list_projects"),
  project: (id: number) => invoke<AnniversaryProject | null>("get_project", { id }),
  createProject: (input: ProjectInput) => invoke<AnniversaryProject>("create_project", { input }),
  updateProject: (id: number, input: ProjectInput) =>
    invoke<void>("update_project", { id, input }),
  deleteProject: (id: number) => invoke<void>("delete_project", { id }),
  addProjectPhoto: (projectId: number, fileName: string, dataBase64: string) =>
    invoke<AnniversaryProject>("add_project_photo", { projectId, fileName, dataBase64 }),
  removeProjectPhoto: (projectId: number, fileName: string) =>
    invoke<void>("remove_project_photo", { projectId, fileName }),
  projectVersions: (projectId: number) =>
    invoke<ProjectVersion[]>("list_project_versions", { projectId }),
  aiProviders: () => invoke<AiProvider[]>("list_ai_providers"),
  createAiProvider: (input: AiProviderInput) =>
    invoke<AiProvider>("create_ai_provider", { input }),
  deleteAiProvider: (id: number) => invoke<void>("delete_ai_provider", { id }),
  generateCopy: (providerId: number, input: AiGenerateInput) =>
    invoke<GeneratedCopy>("generate_copy", { providerId, input }),
  publishTargets: () => invoke<PublishTarget[]>("list_publish_targets"),
  createPublishTarget: (input: PublishTargetInput) =>
    invoke<PublishTarget>("create_publish_target", { input }),
  deletePublishTarget: (id: number) => invoke<void>("delete_publish_target", { id }),
  testPublishTarget: (id: number) => invoke<void>("test_publish_target", { id }),
  publishProject: (projectId: number, targetId: number) =>
    invoke<PublishResult>("publish_project", { projectId, targetId }),
};
