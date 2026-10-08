export type Segment = { start: number; end: number; speaker: string | null; text: string };

export type Transcript = {
  text: string;
  segments: Segment[];
  speech_detected?: boolean;
  suggestion?: string;
};
