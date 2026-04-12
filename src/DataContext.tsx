import { createContext, useContext } from "react";
import { ParsedCSV, ViewSuggestion } from "./types";

interface DataContextValue {
  data: ParsedCSV | null;
  suggestions: ViewSuggestion[];
}

export const DataContext = createContext<DataContextValue>({
  data: null,
  suggestions: [],
});

export function useDataContext() {
  return useContext(DataContext);
}
