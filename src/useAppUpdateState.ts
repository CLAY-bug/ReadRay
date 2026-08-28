import { useSyncExternalStore } from "react";
import {
  appUpdateService,
  type AppUpdateState,
} from "./appUpdateService.ts";

function subscribe(listener: () => void) {
  return appUpdateService.subscribe(listener);
}

function getSnapshot(): AppUpdateState {
  return appUpdateService.getState();
}

export function useAppUpdateState(): AppUpdateState {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
