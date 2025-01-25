// src/components/InstallPWA.tsx
'use client';

import { Button } from 'evergreen-ui';
import { useInstallPrompt } from '@/hooks/useInstallPrompt';

export function InstallPWA() {
  const { isInstallable, isInstalled, promptToInstall } = useInstallPrompt();

  if (!isInstallable || isInstalled) {
    return null;
  }

  return (
    <Button
      appearance="primary"
      intent="success"
      onClick={promptToInstall}
      marginX={8}
      height={32}
    >
      安装应用
    </Button>
  );
}