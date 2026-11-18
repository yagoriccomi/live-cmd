// Ajustes locais do celular (não vão ao PC). Guardados no SecureStore, que o app já usa.
import * as SecureStore from 'expo-secure-store';

export interface Prefs { termFont: number; vibrate: boolean }
export const DEFAULT_PREFS: Prefs = { termFont: 11, vibrate: true };
export const TERM_FONTS = [10, 11, 12, 13, 14];
const KEY = 'prefs';

export async function loadPrefs(): Promise<Prefs> {
  try { return { ...DEFAULT_PREFS, ...JSON.parse((await SecureStore.getItemAsync(KEY)) ?? '{}') }; } catch { return DEFAULT_PREFS; }
}
export function savePrefs(p: Prefs) { SecureStore.setItemAsync(KEY, JSON.stringify(p)).catch(() => {}); }
