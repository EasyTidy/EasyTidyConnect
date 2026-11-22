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
  music: string;
  video: string;
  custom_css: string;
  /** 照片裁剪焦点：文件名 → [x%, y%] */
  photo_focus: Record<string, [number, number]>;
  /** 照片自动放大轮播（默认关闭） */
  slideshow: boolean;
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
  /** 发布目标类型：s3 / r2 / cloudflare_pages 可分享；隧道类为临时链接。 */
  target_kind: string;
};

export type NfcLinkInfo = {
  url: string;
  /** 局域网写入辅助页（手机扫码打开后复制链接） */
  write_page: string;
  bytes: number;
};

export type AboutInfo = {
  name: string;
  version: string;
  tauri_version: string;
  os: string;
  arch: string;
  os_version: string;
  data_dir: string;
  lan_address: string;
  homepage: string;
};

export type CloudflareStatus = {
  connected: boolean;
  /** oauth / token */
  method: string;
  account_id: string;
  account_name: string;
  /** 配置了 OAuth client ID 时可以一键授权 */
  oauth_available: boolean;
  client_id: string;
};

export type CloudflareAccount = { id: string; name: string };

/** 发布目标是否使用已授权的 Cloudflare 账户（标记存在 tunnel_config JSON 里）。 */
export const usesCloudflareAuth = (target: { tunnel_config?: string }) => {
  try {
    return Boolean(JSON.parse(target.tunnel_config || "{}").cf_auth);
  } catch {
    return false;
  }
};

/** 微信 JS-SDK（公众号）配置，AppID / AppSecret 由用户自己填写。 */
export type WechatConfig = {
  enabled: boolean;
  /** server：使用签名服务器，本应用不需要 AppID / AppSecret；local：本机签名 */
  mode: "server" | "local" | "";
  app_id: string;
  app_secret: string;
  /** 签名服务地址，例如 https://sign.example.com/wx/jssdk */
  signer_url: string;
  /** 允许签名的页面域名，逗号分隔 */
  allowed_domains: string;
};

export type ShareSettings = {
  title: string;
  description: string;
};

/** 只有发布到静态托管（S3/R2/Cloudflare Pages）的长期链接才能分享。 */
export const isShareableKind = (kind: string) => kind === "s3" || kind === "r2" || kind === "cloudflare_pages";

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
  /** 内网穿透配置（JSON），见 TunnelConfig */
  tunnel_config: string;
  is_default: boolean;
  created_at: string;
};

export type PublishTargetInput = Omit<PublishTarget, "id" | "created_at">;

/** 与 Rust 端 tunnel::TunnelConfig 字段一致。 */
export type TunnelConfig = {
  public_url: string;
  ngrok_authtoken: string;
  ngrok_domain: string;
  frp_server_addr: string;
  frp_server_port: number;
  frp_token: string;
  frp_proxy_type: "http" | "tcp";
  frp_custom_domain: string;
  frp_remote_port: number;
  frp_http_port: number;
  frpc_path: string;
  ssh_host: string;
  ssh_port: number;
  ssh_user: string;
  ssh_password: string;
  ssh_key: string;
  ssh_key_passphrase: string;
  ssh_remote_port: number;
  bore_server: string;
  bore_port: number;
  bore_secret: string;
};

export const EMPTY_TUNNEL_CONFIG: TunnelConfig = {
  public_url: "",
  ngrok_authtoken: "",
  ngrok_domain: "",
  frp_server_addr: "",
  frp_server_port: 7000,
  frp_token: "",
  frp_proxy_type: "http",
  frp_custom_domain: "",
  frp_remote_port: 0,
  frp_http_port: 80,
  frpc_path: "",
  ssh_host: "",
  ssh_port: 22,
  ssh_user: "",
  ssh_password: "",
  ssh_key: "",
  ssh_key_passphrase: "",
  ssh_remote_port: 0,
  bore_server: "bore.pub",
  bore_port: 0,
  bore_secret: "",
};

export type PublishResult = {
  url: string;
  slug: string;
  uploaded: number;
};

export type TemplateInfo = {
  id: string;
  name: string;
};

export type CustomTemplate = {
  id: number;
  name: string;
  prompt: string;
  css: string;
  dark: boolean;
  created_at: string;
  updated_at: string;
};

/** 项目里 AI 模板的 template_id 形如 `ai-12`。 */
export const customTemplateId = (id: number) => `ai-${id}`;

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
  reorderProjectPhotos: (projectId: number, photos: string[]) =>
    invoke<AnniversaryProject>("reorder_project_photos", { projectId, photos }),
  removeProjectPhoto: (projectId: number, fileName: string) =>
    invoke<void>("remove_project_photo", { projectId, fileName }),
  addProjectMedia: (projectId: number, kind: "music" | "video", fileName: string, dataBase64: string) =>
    invoke<AnniversaryProject>("add_project_media", { projectId, kind, fileName, dataBase64 }),
  removeProjectMedia: (projectId: number, kind: "music" | "video") =>
    invoke<void>("remove_project_media", { projectId, kind }),
  renderPreview: (project: AnniversaryProject) =>
    invoke<string>("render_preview", { project }),
  projectVersions: (projectId: number) =>
    invoke<ProjectVersion[]>("list_project_versions", { projectId }),
  aiProviders: () => invoke<AiProvider[]>("list_ai_providers"),
  createAiProvider: (input: AiProviderInput) =>
    invoke<AiProvider>("create_ai_provider", { input }),
  deleteAiProvider: (id: number) => invoke<void>("delete_ai_provider", { id }),
  generateCopy: (providerId: number, input: AiGenerateInput) =>
    invoke<GeneratedCopy>("generate_copy", { providerId, input }),
  customTemplates: () => invoke<CustomTemplate[]>("list_custom_templates"),
  generateCustomTemplate: (providerId: number, request: string, theme: string, baseId?: number) =>
    invoke<CustomTemplate>("generate_custom_template", {
      providerId,
      request,
      theme,
      baseId: baseId ?? null,
    }),
  renameCustomTemplate: (id: number, name: string) =>
    invoke<CustomTemplate>("rename_custom_template", { id, name }),
  deleteCustomTemplate: (id: number) => invoke<void>("delete_custom_template", { id }),
  nfcLinkInfo: (url: string, name: string) => invoke<NfcLinkInfo>("nfc_link_info", { url, name }),
  aboutInfo: () => invoke<AboutInfo>("get_about_info"),
  openDataDir: () => invoke<void>("open_data_dir"),
  cloudflareStatus: () => invoke<CloudflareStatus>("cloudflare_get_status"),
  cloudflareSetClientId: (clientId: string) => invoke<CloudflareStatus>("cloudflare_set_client_id", { clientId }),
  cloudflareAuthorize: () => invoke<CloudflareStatus>("cloudflare_authorize"),
  cloudflareConnectToken: (token: string) => invoke<CloudflareStatus>("cloudflare_connect_token", { token }),
  cloudflareTokenTemplateUrl: () => invoke<string>("cloudflare_token_template_url"),
  cloudflareAccounts: () => invoke<CloudflareAccount[]>("cloudflare_list_accounts"),
  cloudflareSelectAccount: (account: CloudflareAccount) =>
    invoke<CloudflareStatus>("cloudflare_select_account", { account }),
  cloudflareDisconnect: () => invoke<CloudflareStatus>("cloudflare_disconnect"),
  wechatConfig: () => invoke<WechatConfig>("get_wechat_config"),
  saveWechatConfig: (config: WechatConfig) => invoke<void>("save_wechat_config", { config }),
  testWechat: (config: WechatConfig) => invoke<string>("test_wechat", { config }),
  shareSettings: (projectId: number) => invoke<ShareSettings>("get_share_settings", { projectId }),
  saveShareSettings: (projectId: number, settings: ShareSettings) =>
    invoke<void>("save_share_settings", { projectId, settings }),
  publishTargets: () => invoke<PublishTarget[]>("list_publish_targets"),
  createPublishTarget: (input: PublishTargetInput) =>
    invoke<PublishTarget>("create_publish_target", { input }),
  deletePublishTarget: (id: number) => invoke<void>("delete_publish_target", { id }),
  testPublishTarget: (id: number) => invoke<void>("test_publish_target", { id }),
  publishProject: (projectId: number, targetId: number) =>
    invoke<PublishResult>("publish_project", { projectId, targetId }),
};
