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
};

export type Settings = {
  device_name: string;
  port: number;
  auto_start: boolean;
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
};
