// src/hooks/useInstallPrompt.ts
import { useState, useEffect } from 'react';

interface BeforeInstallPromptEvent extends Event {
  prompt: () => Promise<void>;
  userChoice: Promise<{ outcome: 'accepted' | 'dismissed'; platform: string }>;
}

export function useInstallPrompt() {
  const [prompt, setPrompt] = useState<BeforeInstallPromptEvent | null>(null);
  const [isInstalled, setIsInstalled] = useState(false);

  useEffect(() => {
    const handleBeforeInstallPrompt = (e: Event) => {
      // 阻止 Chrome 67 及更早版本自动显示安装提示
      e.preventDefault();
      // 保存事件
      setPrompt(e as BeforeInstallPromptEvent);
    };

    // 检查是否已经安装
    const checkInstalled = () => {
      const isStandalone = window.matchMedia('(display-mode: standalone)').matches ||
                          (window.navigator as any).standalone ||
                          document.referrer.includes('android-app://');
      setIsInstalled(isStandalone);
    };

    // 监听安装状态变化
    const handleAppInstalled = () => {
      setIsInstalled(true);
      setPrompt(null);
    };

    // 添加事件监听
    window.addEventListener('beforeinstallprompt', handleBeforeInstallPrompt);
    window.addEventListener('appinstalled', handleAppInstalled);
    
    // 初始检查
    checkInstalled();

    // 清理事件监听
    return () => {
      window.removeEventListener('beforeinstallprompt', handleBeforeInstallPrompt);
      window.removeEventListener('appinstalled', handleAppInstalled);
    };
  }, []);

  const promptToInstall = async () => {
    if (!prompt) {
      console.log('Can\'t prompt to install');
      return;
    }

    try {
      // 显示安装提示
      await prompt.prompt();
      // 等待用户响应
      const choiceResult = await prompt.userChoice;
      
      if (choiceResult.outcome === 'accepted') {
        console.log('User accepted the install prompt');
      } else {
        console.log('User dismissed the install prompt');
      }
      
      // 清除提示
      setPrompt(null);
    } catch (error) {
      console.error('Error attempting to prompt to install:', error);
    }
  };

  return {
    isInstallable: !!prompt,
    isInstalled,
    promptToInstall
  };
}