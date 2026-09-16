import { isAppError } from "./types";

/** Tauri rejects with the serialized AppError object; surface its message. */
export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}
