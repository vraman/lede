import { createContext, useContext } from "react";
import { ParsedCSV, ViewSuggestion } from "./types";

interface DataContextValue {
  data: ParsedCSV | null;
  suggestions: ViewSuggestion[];
  sourceFilename: string;
}

export const DataContext = createContext<DataContextValue>({
  data: null,
  suggestions: [],
  sourceFilename: "",
});

export function useDataContext() {
  return useContext(DataContext);
}
