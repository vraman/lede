import { createContext, useContext } from "react";
import { ParsedCSV, ViewSuggestion, AiInsightsResponse } from "./types";

interface DataContextValue {
  data: ParsedCSV | null;
  suggestions: ViewSuggestion[];
  sourceFilename: string;
  aiInsights: AiInsightsResponse | null;
  aiLoading: boolean;
}

export const DataContext = createContext<DataContextValue>({
  data: null,
  suggestions: [],
  sourceFilename: "",
  aiInsights: null,
  aiLoading: false,
});

export function useDataContext() {
  return useContext(DataContext);
}
