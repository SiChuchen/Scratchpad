export type ContentSource = "dock" | "vault";

export type ContentKind =
  | "text"
  | "image"
  | "file"
  | "credential"
  | "bookmark"
  | "note";

export type RetentionState = "temporary" | "saved";

export type BrowseScope = "temporary" | "all" | "saved";

export type ContentOperation =
  | "created"
  | "updated"
  | "retention"
  | "reordered"
  | "deleted"
  | "restored";

export interface ContentCapabilities {
  copyText: boolean;
  copyImage: boolean;
  copyFile: boolean;
  copyPath: boolean;
  openUrl: boolean;
  revealSensitive: boolean;
  edit: boolean;
  save: boolean;
  unsave: boolean;
  delete: boolean;
  reorder: boolean;
}

export interface ContentSummary<K extends ContentKind = ContentKind> {
  id: string;
  kind: K;
  retention: RetentionState;
  title: string;
  preview: string | null;
  createdAt: string;
  updatedAt: string;
  cleanupAt: string | null;
  capabilities: ContentCapabilities;
}

export interface UnifiedField {
  key: string;
  value: string;
  isSensitive: boolean;
  sortOrder: number;
}

export type ContentTagSource = "manual" | "ai";

export interface UnifiedTag {
  tag: string;
  normalizedTag: string;
  source: ContentTagSource;
}

export type ContentDetail =
  | {
      kind: "text";
      summary: ContentSummary<"text">;
      title: string;
      body: string;
    }
  | {
      kind: "image";
      summary: ContentSummary<"image">;
      fileName: string;
      assetPath: string;
      mimeType: string | null;
      width: number | null;
      height: number | null;
      available: boolean;
    }
  | {
      kind: "file";
      summary: ContentSummary<"file">;
      fileName: string;
      assetPath: string;
      mimeType: string | null;
      sizeBytes: number | null;
      available: boolean;
    }
  | {
      kind: "credential";
      summary: ContentSummary<"credential">;
      fields: UnifiedField[];
      notes: string | null;
      tags: UnifiedTag[];
    }
  | {
      kind: "bookmark";
      summary: ContentSummary<"bookmark">;
      url: string;
      fields: UnifiedField[];
      notes: string | null;
      tags: UnifiedTag[];
    }
  | {
      kind: "note";
      summary: ContentSummary<"note">;
      body: string;
      fields: UnifiedField[];
      tags: UnifiedTag[];
    };

export type SearchSource = "local" | "aiExpanded";

export interface ContentSearchHit {
  summary: ContentSummary;
  score: number;
  sources: SearchSource[];
}

export interface UnifiedQueryPlan {
  kinds: ContentKind[];
  keywords: string[];
  aliases: string[];
  dateFrom: string | null;
  dateTo: string | null;
}

export interface ContentChange {
  id: string;
  operation: ContentOperation;
}

export interface ContentChangedEvent {
  revision: number;
  changes: ContentChange[];
}

export interface ContentDeleteFailedEvent {
  token: string;
  id: string;
  code: string;
}

export interface DeleteUndoToken {
  token: string;
  expiresAt: string;
}

export interface ContentRevision {
  revision: number;
}
