export type Origin = "human" | "ai";

export type SummaryLine = { text: string; origin: Origin };

export type SummarySection = { heading: SummaryLine; lines: SummaryLine[] };

export type Summary = { overview: string; sections: SummarySection[]; skipped?: string[] };
