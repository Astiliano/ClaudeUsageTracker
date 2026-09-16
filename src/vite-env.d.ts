/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_MOCK_BACKEND?: string;
}
interface ImportMeta {
  readonly env: ImportMetaEnv;
}
