/// <reference types="vite/client" />
export const API = (import.meta.env.VITE_RFSCOPE_API as string | undefined) ?? "http://127.0.0.1:8787/api/v1";
export const SPECTRUM_WS = API.replace(/^http/, "ws") + "/stream/spectrum";
