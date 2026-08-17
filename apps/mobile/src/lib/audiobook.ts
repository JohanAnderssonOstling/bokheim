import type { AudiobookChapter } from "./bindings";

export type MobileAudiobookSession = {
  key: string;
  libraryId: string;
  contentHash: string;
  source: string;
  nativeSource: string;
  nativeKey: string;
  title: string;
  author: string;
  coverUrl: string;
  initialPositionMs: number;
  expectedDurationMs: number | null;
  chapters: AudiobookChapter[];
  savePosition: (positionMs: number, progress: number) => Promise<void>;
};
