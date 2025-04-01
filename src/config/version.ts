// src/config/version.ts

// 确保在类型检查时不会出错的版本号获取
// 通过类型断言来避免潜在的类型错误
const getEnvVar = (key: string, fallback: string): string => {
    try {
      const value = (import.meta.env as any)[key];
      return value !== undefined ? value : fallback;
    } catch {
      return fallback;
    }
  };
  
  // 从Vite注入的环境变量中获取版本号
  export const APP_VERSION = getEnvVar('APP_VERSION', '0.0.0');
  
  // 构建时间，从环境变量中获取
  export const BUILD_DATE = getEnvVar('BUILD_DATE', new Date().toISOString());
  
  // 格式化显示的版本信息
  export const getVersionDisplay = (): string => {
    return `v${APP_VERSION}`;
  };