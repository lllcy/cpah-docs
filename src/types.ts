export type DeletePolicy = "trash" | "keep" | "delete";
export type TagSelectionMode = "single" | "multiple";

export interface CategoryLabel {
  id: string;
  name: string;
  description: string;
}

export interface TaggingConfig {
  enabled: boolean;
  selectionMode: TagSelectionMode;
  labels: CategoryLabel[];
}

export interface WatchProfile {
  id: string;
  name: string;
  inputDir: string;
  outputDir: string;
  enabled: boolean;
  deletePolicy: DeletePolicy;
  tagging: TaggingConfig;
}

export interface AgentSettings {
  baseUrl: string;
  model: string;
  configured: boolean;
  concurrency: number;
}

export interface AppSettings {
  profiles: WatchProfile[];
  monitoringPaused: boolean;
  paused: boolean;
  classificationPaused: boolean;
  mineruBaseUrl: string;
  mineruConfigured: boolean;
  enabledExtensions: string[];
  agent: AgentSettings;
}

export type JobStatus =
  | "waiting_stable"
  | "queued"
  | "converting"
  | "waiting_mineru"
  | "uploading"
  | "processing"
  | "downloading"
  | "completed"
  | "failed";

export interface TaskRecord {
  id: string;
  profileId: string;
  sourcePath: string;
  relativePath: string;
  sourceHash?: string;
  sourceSize?: number;
  sourceModifiedMs?: number;
  engine: "anytomd" | "mineru";
  status: JobStatus;
  outputPath?: string;
  error?: string;
  errorCode?: string;
  errorTitle?: string;
  errorSuggestion?: string;
  mineruState?: string;
  mineruExtractedPages?: number;
  mineruTotalPages?: number;
  mineruStartedAt?: string;
  updatedAt: string;
  tagJobId?: string;
  tagStatus?: TagJobStatus;
}

export type TagJobStatus = "queued" | "reading" | "writing" | "completed" | "failed" | "outdated" | "cancelled";

export interface TagJobRecord {
  id: string;
  profileId: string;
  markdownPath: string;
  relativePath: string;
  status: TagJobStatus;
  contentHash?: string;
  schemaHash: string;
  resultJson?: string;
  error?: string;
  errorCode?: string;
  errorTitle?: string;
  errorSuggestion?: string;
  readBytes: number;
  totalBytes: number;
  apiCalls: number;
  inputTokens: number;
  outputTokens: number;
  updatedAt: string;
}

export interface TaggingImpact {
  discovered: number;
  newFiles: number;
  affected: number;
}

export interface Dashboard {
  settings: AppSettings;
  tasks: TaskRecord[];
  tagJobs: TagJobRecord[];
  taskTotal: number;
  tagJobTotal: number;
  runtimeError: string | null;
  tagRuntimeError: string | null;
  indexRuntimeError: string | null;
}

export type HealthLevel = "ok" | "warning" | "error";

export interface HealthCheck {
  id: string;
  title: string;
  level: HealthLevel;
  detail: string;
  suggestion?: string;
}

export interface HealthReport {
  appVersion: string;
  checkedAt: string;
  overall: HealthLevel;
  checks: HealthCheck[];
  counts: {
    conversionPending: number;
    conversionActive: number;
    conversionWaitingMineru: number;
    conversionFailed: number;
    classificationPending: number;
    classificationActive: number;
    classificationFailed: number;
    classificationOutdated: number;
  };
}
