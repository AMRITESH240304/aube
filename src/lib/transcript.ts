export type Segment = { start: number; end: number; speaker: string | null; text: string };

export type Transcript = { segments: Segment[] };
