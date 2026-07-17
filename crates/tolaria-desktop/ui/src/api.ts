import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type {
  DeckFile,
  DeckInfo,
  Limit,
  MetaResponse,
  PoolInfo,
  ProgressPayload,
  ReplayDto,
  ReplayRequest,
  RunConfig,
  RunMeta,
  RunResult,
} from "./types";

export const api = {
  poolStatus: () => invoke<PoolInfo>("pool_status"),
  parseDeck: (text: string, name: string) => invoke<DeckInfo>("parse_deck", { text, name }),
  saveDeck: (name: string, text: string) => invoke<void>("save_deck", { name, text }),
  listDecks: () => invoke<DeckFile[]>("list_decks"),
  deleteDeck: (name: string) => invoke<void>("delete_deck", { name }),
  fetchMeta: (format: string, days: number, top: number, selection: string) =>
    invoke<MetaResponse>("fetch_meta", { format, days, top, selection }),
  startRun: (config: RunConfig) => invoke<void>("start_run", { config }),
  cancelRun: () => invoke<void>("cancel_run"),
  listRuns: () => invoke<RunMeta[]>("list_runs"),
  loadRun: (file: string) => invoke<RunResult>("load_run", { file }),
  listLimits: () => invoke<Limit[]>("list_limits"),
  replayGame: (req: ReplayRequest) => invoke<ReplayDto>("replay_game", { req }),
};

export function onRunProgress(cb: (p: ProgressPayload) => void): Promise<UnlistenFn> {
  return listen<ProgressPayload>("run-progress", (e) => cb(e.payload));
}

export function onRunDone(cb: (r: RunResult) => void): Promise<UnlistenFn> {
  return listen<RunResult>("run-done", (e) => cb(e.payload));
}

export function onRunError(cb: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("run-error", (e) => cb(e.payload));
}

export function onMetaProgress(cb: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("meta-progress", (e) => cb(e.payload));
}
