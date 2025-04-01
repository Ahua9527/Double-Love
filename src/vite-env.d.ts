/// <reference types="vite/client" />

interface ImportMeta {
    readonly env: {
      readonly MODE: string;
      readonly BASE_URL: string;
      readonly PROD: boolean;
      readonly DEV: boolean;
      readonly SSR: boolean;
      
      // 添加我们自定义的环境变量
      readonly APP_VERSION: string;
      readonly BUILD_DATE: string;
    }
  }