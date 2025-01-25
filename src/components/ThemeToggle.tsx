//components/ThemeToggle.tsx
import React from 'react';
import { useTheme } from '@/hooks/useTheme';
import { Sun, Moon } from 'lucide-react';
import { Button } from 'evergreen-ui';

export const ThemeToggle = () => {
  const { theme, toggleTheme } = useTheme();
  
  return (
    <Button
      appearance="minimal"
      onClick={toggleTheme}
      className="fixed top-4 right-4"
    >
      {theme === 'dark' ? (
        <Sun size={20} className="text-gray-200" />
      ) : (
        <Moon size={20} className="text-gray-600" />
      )}
    </Button>
  );
};