import { createContext, useContext } from "react";
import { ParsedCSV, ViewSuggestion } from "./types";

interface DataContextValue {
  data: ParsedCSV | null;
  suggestions: ViewSuggestion[];
  sourceFilename: string;
  aiLoading: boolean;
  duckDbReady: boolean;
}

export const DataContext = createContext<DataContextValue>({
  data: null,
  suggestions: [],
  sourceFilename: "",
  aiLoading: false,
  duckDbReady: false,
});

export function useDataContext() {
  return useContext(DataContext);
}
