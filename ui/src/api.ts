// Types and calls to the backend (Tauri commands). Graph ids (u64) travel as strings.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type NodeKind =
  | "personality_root"
  | "trait"
  | "emotion"
  | "value"
  | "goal"
  | "episodic_memory"
  | "semantic_fact"
  | "entity"
  | "place"
  | "topic"
  | "world_root"
  | "character"
  | "faction"
  | "lore"
  | "event"
  | "appearance"
  | "chat_root"
  | "concept"
  | "library_root";

export type EdgeKind = "composes" | "involves" | "related_to" | "causes" | "precedes" | "evokes" | "knows" | "contradicts";

/**
 * A point of the story, in in-world days. The open ends travel as words: "always" (a
 * node or link true since forever), "open" (a link that still holds) and "present" (the
 * now of a world without a timeline).
 */
export type Story = number | "always" | "open" | "present";

/** A moment of a world's timeline. `id` 0 = a new one (the backend assigns it). */
export interface MomentDto {
  id: number;
  name: string;
  year: number;
  day: number;
  summary: string;
  /** Story time (in-world days); set by the backend. */
  at: number;
}

/** A moment a character belongs to, with their scenario then (empty = the usual one). */
export interface CharacterMomentDto {
  moment: number;
  scenario: string;
}

export interface CharacterDto {
  id: string;
  name: string;
  tagline: string;
  appearance: string;
  avatar?: string | null;
  nodes: number;
  /** Their usual scenario. */
  scenario: string;
  /** Moments of the story they belong to (empty = any). */
  moments: CharacterMomentDto[];
  /** In a Collab, the key and name of the world they come from. */
  world?: string | null;
  worldName?: string | null;
}

/** A world a Collab links, at a moment of its timeline (null = the latest). */
export interface CollabLink {
  world: string;
  moment: number | null;
}

/** How a world's chats look (its style.toml). */
export interface WorldStyle {
  /** `#rrggbb`. */
  accent: string;
  mode: "dark" | "light";
  font: "serif" | "sans";
  texture: "none" | "grain" | "paper";
}

export interface WorldDto {
  key: string;
  category: string;
  name: string;
  premise: string;
  nodes: number;
  characters: CharacterDto[];
  /** In story order. */
  timeline: MomentDto[];
  style: WorldStyle;
  /** Its libraries of shared definitions (category's, universal), existing or not. */
  definitions: { key: string; name: string; nodes: number }[];
  /** Only for a Collab (`key` = "collab:<slug>"): the worlds it links. */
  links?: CollabLink[] | null;
}

export interface MessageDto {
  speaker: string;
  speakerId?: string | null;
  text: string;
  at: number;
}

export interface ChatSummary {
  id: string;
  title: string;
  participants: string[];
  userName: string;
  updatedAt: number;
  messageCount: number;
  lastSpeaker?: string | null;
  lastText?: string | null;
}

export interface ChatDto {
  id: string;
  title: string;
  participants: string[];
  userName: string;
  messages: MessageDto[];
  /** Whether each character's own memory also learns from this chat. */
  charactersRemember: boolean;
  /** Moment of the world's timeline (null = the latest). */
  moment?: number | null;
}

/** Which memory graph: the world's, a chat's or a character's. */
export type Scope = "character" | "world" | "chat" | "library" | "collab";

export interface ViewNode {
  id: number;
  name: string;
  kind: number;
  val: number;
  valence: number;
  heat?: number;
}

export interface ViewLink {
  source: number;
  target: number;
  kind: number;
  weight: number;
  /** The link no longer holds from some point of the story. */
  closed?: boolean;
}

export interface NodeDto {
  id: number;
  kind: NodeKind;
  kindCode: number;
  label: string;
  content: string;
  weight: number;
  valence: number;
  createdAt: number;
  lastAccessed: number;
  accessCount: number;
  degree: number;
  isRoot: boolean;
  linked: boolean;
  /** From when it exists in the story. */
  since: Story;
}

export interface GraphDto {
  name: string;
  scope: Scope;
  view: { root: number; nodes: ViewNode[]; links: ViewLink[] };
  nodes: NodeDto[];
}

export interface EdgeDetail {
  other: number;
  otherLabel: string;
  kind: EdgeKind;
  weight: number;
  outgoing: boolean;
  validFrom: Story;
  /** "open" while it still holds. */
  validUntil: Story;
}

export interface SlotStatus {
  slot: "actor" | "curator" | "reader";
  running: boolean;
  model: string;
  vision: boolean;
  gpu_mb: number;
  url: string;
}

export interface ModelStatus {
  slots: SlotStatus[];
  home: string;
  imageReady: boolean;
  imageInfo: string;
  visionError?: string | null;
}

export interface NodeEdit {
  kind?: NodeKind;
  label?: string;
  content?: string;
  weight?: number;
  valence?: number;
  since?: Story;
}

export interface Aspect {
  label: string;
  content: string;
  weight: number;
}

export interface AppearanceDto {
  canonical: Aspect[];
  seenBy: { by: string; aspects: Aspect[] }[];
  references: { file: string; dataUrl: string }[];
  prompt: string;
  avatar?: string | null;
  warning?: string | null;
}

export interface NewNode {
  kind: NodeKind;
  label: string;
  content: string;
  weight: number;
  valence: number;
  linkFrom?: number | null;
  edgeKind?: EdgeKind | null;
}

export const api = {
  listWorlds: () => invoke<WorldDto[]>("list_worlds"),
  /** Collabs, shaped like worlds: their key works wherever a world's does for chats and memory. */
  listCollabs: () => invoke<WorldDto[]>("list_collabs"),
  createCollab: (name: string, premise: string, links: CollabLink[]) => invoke<WorldDto>("create_collab", { req: { name, premise, links } }),
  setCollabLinks: (collab: string, links: CollabLink[]) => invoke<WorldDto>("set_collab_links", { collab, links }),
  listChats: (world: string) => invoke<ChatSummary[]>("list_chats", { world }),
  createChat: (world: string, participants: string[], title: string, userName: string, charactersRemember: boolean, moment: number | null) =>
    invoke<ChatDto>("create_chat", { req: { world, participants, title, userName, charactersRemember, moment } }),
  setChatMoment: (world: string, id: string, moment: number | null) => invoke<ChatDto>("set_chat_moment", { world, id, moment }),
  setTimeline: (world: string, moments: MomentDto[]) => invoke<MomentDto[]>("set_timeline", { world, moments }),
  setWorldStyle: (world: string, style: WorldStyle) => invoke<WorldStyle>("set_world_style", { world, style }),
  setCharacterMoments: (world: string, character: string, moments: CharacterMomentDto[]) =>
    invoke<void>("set_character_moments", { world, character, moments }),
  setChatMemoryMode: (world: string, id: string, charactersRemember: boolean) =>
    invoke<ChatDto>("set_chat_memory_mode", { world, id, charactersRemember }),
  getChat: (world: string, id: string) => invoke<ChatDto>("get_chat", { world, id }),
  renameChat: (world: string, id: string, title: string) => invoke<void>("rename_chat", { world, id, title }),
  deleteChat: (world: string, id: string) => invoke<void>("delete_chat", { world, id }),
  send: (world: string, id: string, text: string, speaker: string | null) =>
    invoke<MessageDto>("send_message", { req: { world, id, text, speaker } }),
  lastContext: (id: string) => invoke<string | null>("last_context", { id }),
  modelStatus: () => invoke<ModelStatus>("model_status"),
  stopModels: () => invoke<void>("stop_models"),
  portrait: (world: string, character: string, scene: string) => invoke<AppearanceDto>("generate_portrait", { world, character, scene }),
  appearance: (world: string, character: string) => invoke<AppearanceDto>("appearance", { world, character }),
  setAppearance: (world: string, character: string, aspects: Aspect[]) => invoke<AppearanceDto>("set_appearance", { world, character, aspects }),
  addReference: (world: string, character: string, fileName: string, dataBase64: string) =>
    invoke<AppearanceDto>("add_reference", { req: { world, character, fileName, dataBase64 } }),
  /** The vision reader's proposal from all the references (it is not saved by itself). */
  describeReferences: (world: string, character: string) => invoke<Aspect[]>("describe_references", { world, character }),
  deleteReference: (world: string, character: string, file: string) => invoke<AppearanceDto>("delete_reference", { world, character, file }),

  graph: (world: string, graph: string) => invoke<GraphDto>("memory_graph", { world, graph }),
  edges: (world: string, graph: string, node: number) => invoke<EdgeDetail[]>("memory_edges", { world, graph, node }),
  updateNode: (world: string, graph: string, node: number, edit: NodeEdit) => invoke<void>("memory_update_node", { world, graph, node, edit }),
  createNode: (world: string, graph: string, node: NewNode) => invoke<number>("memory_create_node", { world, graph, node }),
  deleteNode: (world: string, graph: string, node: number) => invoke<void>("memory_delete_node", { world, graph, node }),
  setEdge: (world: string, graph: string, from: number, to: number, kind: EdgeKind, weight: number) =>
    invoke<void>("memory_set_edge", { world, graph, from, to, kind, weight }),
  /** Deletes the open link, or the closed one that ends at `until`. */
  deleteEdge: (world: string, graph: string, from: number, to: number, until: Story = "open") =>
    invoke<void>("memory_delete_edge", { world, graph, from, to, until }),
  /** When the link `from → to` that now ends at `until` holds. */
  setEdgeValidity: (world: string, graph: string, from: number, to: number, until: Story, validFrom: Story, validUntil: Story) =>
    invoke<void>("memory_set_edge_validity", { world, graph, from, to, until, validFrom, validUntil }),
  probe: (world: string, graph: string, text: string) => invoke<Record<string, number>>("memory_probe", { world, graph, text }),
};

export interface TokenEvent {
  conversation: string;
  speakerId: string;
  speaker: string;
  token: string;
}

export interface StatusEvent {
  text: string;
  busy: boolean;
}

export interface MemoryEvent {
  world: string;
  graph: string;
  name: string;
  applied: number;
  rejected: number;
  error?: string | null;
}

export const on = {
  token: (f: (e: TokenEvent) => void): Promise<UnlistenFn> => listen<TokenEvent>("chat-token", (e) => f(e.payload)),
  status: (f: (e: StatusEvent) => void): Promise<UnlistenFn> => listen<StatusEvent>("status", (e) => f(e.payload)),
  memory: (f: (e: MemoryEvent) => void): Promise<UnlistenFn> => listen<MemoryEvent>("memory-updated", (e) => f(e.payload)),
};

/** Readable message of an `invoke` error (the commands return `String`). */
export function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
