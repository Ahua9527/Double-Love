'use client'

import { 
  Pane, 
  FileUploader, 
  FileCard, 
  Alert, 
  Heading, 
  Button,
  TextInput,
  Label,
  majorScale,
  Text,
  Strong,
  MimeType
} from 'evergreen-ui'
import React, { useEffect, useState, useCallback } from 'react'
import { processXML } from '@/utils/xml'
import { ThemeToggle } from '@/components/ThemeToggle'
import { useTheme } from '@/hooks/useTheme'

type ProcessError = {
  type: 'upload' | 'process' | 'download';
  message: string;
  fileName: string;
}

export default function Home() {
  const { theme } = useTheme();
  const [isMounted, setIsMounted] = useState(false)
  const [files, setFiles] = useState<File[]>([])
  const [fileRejections, setFileRejections] = useState<{ file: File; message: string }[]>([])
  const [isProcessing, setIsProcessing] = useState(false)
  const [errors, setErrors] = useState<ProcessError[]>([])
  const [width, setWidth] = useState('1920')
  const [height, setHeight] = useState('1080')
  const [prefix, setPrefix] = useState('')

  const validateResolution = useCallback((value: string): boolean => {
    const num = parseInt(value)
    return !isNaN(num) && num > 0 && num <= 8192
  }, [])

  const addError = useCallback((error: ProcessError) => {
    setErrors(prev => [...prev, error])
  }, [])

  useEffect(() => {
    setIsMounted(true)
    return () => {
      files.forEach(file => {
        URL.revokeObjectURL(URL.createObjectURL(file))
      })
      setIsMounted(false)
    }
  }, [files])

  if (!isMounted) {
    return null
  }

  const handleAccepted = (acceptedFiles: File[]) => {
    setFiles(prev => [...prev, ...acceptedFiles])
    setErrors([])
  }

  const handleRejected = (rejectedFiles: { file: File; message: string }[]) => {
    setFileRejections(rejectedFiles)
    rejectedFiles.forEach(rejection => {
      addError({
        type: 'upload',
        fileName: rejection.file.name,
        message: rejection.message
      })
    })
  }

  const handleRemove = (file: File) => {
    setFiles(files.filter(f => f !== file))
    setFileRejections(fileRejections.filter(r => r.file !== file))
    setErrors(errors.filter(e => e.fileName !== file.name))
  }

  const handleResolutionChange = (
    e: React.ChangeEvent<HTMLInputElement>,
    setter: (value: string) => void
  ) => {
    const value = e.target.value
    if (value === '' || validateResolution(value)) {
      setter(value)
    }
  }

  const handleProcess = async () => {
    setIsProcessing(true)
    setErrors([])
    
    try {
      const successfulFiles: File[] = [];
      
      for (const file of files) {
        try {
          const processedXML = await processXML(file, {
            width: parseInt(width),
            height: parseInt(height),
            format: '{scene}_{shot}_{take}{camera}_{Rating}',
            prefix
          });

          const blob = new Blob([processedXML], { type: 'text/xml;charset=utf-8' });
          const url = URL.createObjectURL(blob);
          
          await new Promise(resolve => setTimeout(resolve, 500));
          
          const a = document.createElement('a');
          a.href = url;
          a.download = file.name.replace('.xml', '_Double_LOVE.xml');
          document.body.appendChild(a);
          a.click();
          
          await new Promise(resolve => setTimeout(resolve, 500));
          document.body.removeChild(a);
          URL.revokeObjectURL(url);
          
          successfulFiles.push(file);
        } catch (err) {
          addError({
            type: 'process',
            fileName: file.name,
            message: err instanceof Error ? err.message : '处理失败'
          });
        }
      }

      setFiles(prev => prev.filter(f => !successfulFiles.includes(f)));
      
    } catch (err) {
      addError({
        type: 'process',
        fileName: 'batch',
        message: err instanceof Error ? err.message : '批量处理失败'
      })
    } finally {
      setIsProcessing(false)
    }
  }

  const renderErrors = () => {
    const errorsByType = {
      upload: errors.filter(e => e.type === 'upload'),
      process: errors.filter(e => e.type === 'process'),
      download: errors.filter(e => e.type === 'download')
    }

    return Object.entries(errorsByType).map(([type, typeErrors]) => {
      if (typeErrors.length === 0) return null
      return (
        <Alert
          key={type}
          intent="danger"
          title={`${type === 'upload' ? '上传' : type === 'process' ? '处理' : '下载'}错误`}
          marginBottom={majorScale(2)}
        >
          {typeErrors.map(err => (
            <div key={err.fileName}>{err.fileName}: {err.message}</div>
          ))}
        </Alert>
      )
    })
  }

  return (
    <Pane 
      display="flex" 
      alignItems="center" 
      justifyContent="center" 
      minHeight="100vh"
      padding={24}
      background={theme === 'dark' ? '#0a0a0a' : 'tint1'}
      className="transition-colors duration-200"
    >
      <ThemeToggle />
      <Pane 
        elevation={4}
        float="left"
        margin={24}
        width="100%"
        maxWidth={720}
        background={theme === 'dark' ? '#1a1a1a' : 'white'}
        padding={48}
        borderRadius={8}
        borderColor={theme === 'dark' ? '#2d2d2d' : undefined}
        className="transition-colors duration-200"
      >
        <Pane 
          display="flex" 
          flexDirection="column" 
          alignItems="center" 
          marginBottom={32}
        >
          <Heading size={900} color={theme === 'dark' ? 'white' : undefined}>Double-LOVE</Heading>
        </Pane>

        <Pane 
          background={theme === 'dark' ? '#262626' : 'tint2'}
          padding={24} 
          borderRadius={8}
          marginBottom={16}
          borderColor={theme === 'dark' ? '#2d2d2d' : undefined}
          className="transition-colors duration-200"
        >
          <Pane marginBottom={16}>
            <Label>
              <Strong color={theme === 'dark' ? 'white' : undefined}>自定义前缀</Strong>
            </Label>
            <TextInput
              value={prefix}
              onChange={(e) => setPrefix(e.target.value)}
              width="100%"
              height={40}
              placeholder="输入自定义前缀（可选）"
              background={theme === 'dark' ? '#1a1a1a' : 'white'}
              borderColor={theme === 'dark' ? '#2d2d2d' : undefined}
            />
          </Pane>

          <Pane display="flex" gap={16}>
            <Pane flex={1}>
              <Label>
                <Strong color={theme === 'dark' ? 'white' : undefined}>分辨率 - 宽度</Strong>
              </Label>
              <TextInput
                value={width}
                onChange={(e) => handleResolutionChange(e, setWidth)}
                height={40}
                width="100%"
                placeholder="宽度"
                background={theme === 'dark' ? '#1a1a1a' : 'white'}
                borderColor={theme === 'dark' ? '#2d2d2d' : undefined}
              />
            </Pane>
            <Pane display="flex" alignItems="center" marginTop={24}>
              <Text color={theme === 'dark' ? 'white' : undefined}>×</Text>
            </Pane>
            <Pane flex={1}>
              <Label>
                <Strong color={theme === 'dark' ? 'white' : undefined}>分辨率 - 高度</Strong>
              </Label>
              <TextInput
                value={height}
                onChange={(e) => handleResolutionChange(e, setHeight)}
                height={40}
                width="100%"
                placeholder="高度"
                background={theme === 'dark' ? '#1a1a1a' : 'white'}
                borderColor={theme === 'dark' ? '#2d2d2d' : undefined}
              />
            </Pane>
          </Pane>
        </Pane>

        <Pane
          background={files.length === 0 ? (theme === 'dark' ? '#262626' : 'tint2') : (theme === 'dark' ? '#1a1a1a' : 'white')}
          padding={24}
          borderRadius={8}
          border="default"
          borderColor={theme === 'dark' ? '#2d2d2d' : undefined}
          className="transition-colors duration-200"
        >
          <FileUploader
            label={<Strong color={theme === 'dark' ? 'white' : undefined}>上传 XML 文件</Strong>}
            description={
              <Text color={theme === 'dark' ? '#a6a6a6' : undefined}>
                支持拖放或点击上传，单个文件最大 50MB
              </Text>
            }
            maxSizeInBytes={50 * 1024 ** 2}
            acceptedMimeTypes={['text/xml' as MimeType]}
            onAccepted={handleAccepted}
            onRejected={handleRejected}
            disabled={isProcessing}
            maxFiles={10}
            background={theme === 'dark' ? '#1a1a1a' : undefined}
            borderColor={theme === 'dark' ? '#2d2d2d' : undefined}
          />
          
          {files.length > 0 && (
            <Pane marginTop={16}>
              <Pane 
                display="flex" 
                justifyContent="space-between" 
                alignItems="center"
                marginBottom={8}
              >
                <Strong color={theme === 'dark' ? 'white' : undefined}>已上传文件 ({files.length})</Strong>
                <Button 
                  height={24} 
                  appearance="minimal" 
                  onClick={() => setFiles([])}
                  color={theme === 'dark' ? 'white' : undefined}
                >
                  清空
                </Button>
              </Pane>
              {files.map((file, index) => (
                <FileCard
                  key={`${file.name}-${index}`}
                  name={file.name}
                  sizeInBytes={file.size}
                  onRemove={() => handleRemove(file)}
                  marginBottom={8}
                  background={theme === 'dark' ? '#262626' : undefined}
                  borderColor={theme === 'dark' ? '#2d2d2d' : undefined}
                />
              ))}
            </Pane>
          )}
        </Pane>

        {files.length > 0 && (
          <Button
            appearance="primary"
            intent="success"
            onClick={handleProcess}
            isLoading={isProcessing}
            disabled={isProcessing}
            height={48}
            width="100%"
            marginTop={16}
          >
            {isProcessing ? `正在处理 ${files.length} 个文件...` : `处理 ${files.length} 个文件`}
          </Button>
        )}

        {errors.length > 0 && (
          <Pane marginTop={16}>
            {renderErrors()}
          </Pane>
        )}
      </Pane>
    </Pane>
  )
}