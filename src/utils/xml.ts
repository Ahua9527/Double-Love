// 类型定义
interface XMLProcessConfig {
  width?: number;
  height?: number;
  format?: string;
  prefix?: string;
}

enum XMLProcessErrorType {
  INVALID_XML = 'INVALID_XML',
  MISSING_REQUIRED_ELEMENTS = 'MISSING_REQUIRED_ELEMENTS',
  INVALID_FORMAT = 'INVALID_FORMAT'
}

class XMLProcessError extends Error {
  constructor(
    public type: XMLProcessErrorType,
    message: string
  ) {
    super(message);
    this.name = 'XMLProcessError';
  }
}

interface ClipElements {
  logginginfo: Element;
  scene: Element;
  shottake: Element;
  filmdata: Element;
  comments: Element;
  mastercomment2: Element;
}

interface ProcessedClipData {
  sceneFormatted: string;
  shotFormatted: string;
  takeFormatted: string;
  cameraId: string;
  rating: string;
}

// 常量定义
const DEFAULT_CONFIG = {
  width: 1920,
  height: 1080,
  format: '{scene}_{shot}_{take}{camera}{Rating}',
  prefix: ''
} as const;

// 工具函数
function isValidValue(value: string): boolean {
  value = value.trim();
  if (!value) return false;
  if (/^-+$/.test(value)) return false;
  if (/^\s*-\s*$/.test(value)) return false;
  return true;
}

function formatSceneNumber(scene: string): string {
  return scene.replace(/(\d+)/g, match => match.padStart(3, '0')).toUpperCase();
}

function formatShotTake(value: string): [string, string] {
  const [shot, take] = value.replace(/\s/g, '').split('-');
  return [
    shot.replace(/(\d+)/g, match => match.padStart(2, '0')).toLowerCase(),
    take.replace(/(\d+)/g, match => match.padStart(2, '0')).toLowerCase()
  ];
}

function cleanupFileName(name: string): string {
  return name
    .replace(/_{2,}/g, '_')
    .replace(/_+$/, '');
}

function getRatingValue(comment: string): string {
  if (comment.includes("Circle")) return "ok";
  if (comment.includes("KEEP")) return "kp";
  if (comment.includes("NG")) return "ng";
  return "";
}

// 主要处理函数
export function getCameraIdentifier(camerarollText: string): string {
  if (!camerarollText) return "";
  
  const match = camerarollText.match(/^[A-Za-z]+/);
  if (!match) return "";
  
  const letters = match[0];
  
  if (letters.length > 2) {
    return letters.slice(0, 2).toLowerCase();
  }
  
  return letters.toLowerCase();
}

function extractClipElements(clip: Element): ClipElements | null {
  const logginginfo = clip.querySelector('logginginfo');
  const scene = logginginfo?.querySelector('scene');
  const shottake = logginginfo?.querySelector('shottake');
  const filmdata = clip.querySelector('filmdata');
  const comments = clip.querySelector('comments');
  const mastercomment2 = comments?.querySelector('mastercomment2');
  
  if (!logginginfo || !scene || !shottake || !filmdata || !comments || !mastercomment2) {
    return null;
  }
  
  return { logginginfo, scene, shottake, filmdata, comments, mastercomment2 };
}

function processClipData(elements: ClipElements): ProcessedClipData | null {
  const { scene, shottake, filmdata, mastercomment2 } = elements;
  
  const sceneValue = scene.textContent || "";
  const shottakeValue = shottake.textContent || "";
  
  if (!isValidValue(sceneValue) || !isValidValue(shottakeValue)) {
    return null;
  }
  
  const sceneFormatted = formatSceneNumber(sceneValue);
  const [shotFormatted, takeFormatted] = formatShotTake(shottakeValue);
  
  const cameraroll = filmdata.querySelector('cameraroll');
  if (!cameraroll?.textContent) return null;
  
  const cameraId = getCameraIdentifier(cameraroll.textContent);
  const mastercomment2Value = (mastercomment2.textContent?.trim() || "").replace(/,$/, '');
  const rating = getRatingValue(mastercomment2Value);
  
  return {
    sceneFormatted,
    shotFormatted,
    takeFormatted,
    cameraId,
    rating
  };
}

function generateNewName(data: ProcessedClipData, config: Required<XMLProcessConfig>): string {
  let newName = config.format
    .replace('{scene}', data.sceneFormatted)
    .replace('{shot}', data.shotFormatted)
    .replace('{take}', data.takeFormatted)
    .replace('{camera}', data.cameraId)
    .replace('{Rating}', data.rating ? `_${data.rating}` : '');
    
  newName = config.prefix + cleanupFileName(newName);
  
  return newName;
}

function updateRelatedElements(clip: Element, xmlDoc: Document, newName: string): void {
  const clipId = clip.getAttribute('id');
  if (!clipId) return;
  
  // 更新clip name
  const nameElem = clip.querySelector('name');
  if (nameElem) {
    nameElem.textContent = newName;
  }
  
  // 更新sequence相关元素
  const sequenceElem = xmlDoc.querySelector(`sequence[id="sequence_id_${clipId}"]`) ||
                      xmlDoc.querySelector(`sequence[id="sequence_${clipId}_ci"]`);
  
  if (sequenceElem) {
    // 更新sequence name
    const sequenceName = sequenceElem.querySelector('name');
    if (sequenceName) {
      sequenceName.textContent = newName;
    }
    
    // 更新第三个name元素
    const nameElements = sequenceElem.getElementsByTagName('name');
    if (nameElements.length >= 3) {
      nameElements[2].textContent = newName;
    }
    
    // 更新clipitem
    const clipitem = sequenceElem.querySelector(`clipitem[id="sequence_${clipId}_ci"]`);
    if (clipitem) {
      const clipitemName = clipitem.querySelector('name');
      if (clipitemName) {
        clipitemName.textContent = newName;
      }
    }
  }
}

function updateResolution(xmlDoc: Document, config: Required<XMLProcessConfig>): void {
  const widthElems = xmlDoc.getElementsByTagName('width');
  const heightElems = xmlDoc.getElementsByTagName('height');
  
  Array.from(widthElems).forEach(elem => {
    elem.textContent = config.width.toString();
  });
  
  Array.from(heightElems).forEach(elem => {
    elem.textContent = config.height.toString();
  });
}

function updateDITInfo(xmlDoc: Document): void {
  const lognoteElems = xmlDoc.getElementsByTagName('lognote');
  Array.from(lognoteElems).forEach(elem => {
    if (elem.textContent === 'DIT: (null)') {
      elem.textContent = 'DIT: 哆啦Ahua 🌱';
    }
  });
}

// 主函数
export async function processXML(file: File, config?: XMLProcessConfig): Promise<string> {
  // 合并配置
  const finalConfig = { ...DEFAULT_CONFIG, ...config };
  
  // 解析XML
  const text = await file.text();
  const parser = new DOMParser();
  const xmlDoc = parser.parseFromString(text, 'text/xml');
  
  // 验证XML
  if (xmlDoc.getElementsByTagName('parsererror').length > 0) {
    throw new XMLProcessError(
      XMLProcessErrorType.INVALID_XML,
      '无效的XML文件'
    );
  }
  
  // 处理所有clip元素
  const clips = xmlDoc.getElementsByTagName('clip');
  
  for (const clip of Array.from(clips)) {
    try {
      // 提取必要元素
      const elements = extractClipElements(clip);
      if (!elements) continue;
      
      // 处理数据
      const processedData = processClipData(elements);
      if (!processedData) continue;
      
      // 生成新名称
      const newName = generateNewName(processedData, finalConfig);
      
      // 更新相关元素
      updateRelatedElements(clip, xmlDoc, newName);
      
    } catch (error) {
      console.error('处理clip失败:', error);
      continue;
    }
  }
  
  // 更新分辨率
  updateResolution(xmlDoc, finalConfig);
  
  // 更新DIT信息
  updateDITInfo(xmlDoc);
  
  // 序列化并返回
  const serializer = new XMLSerializer();
  return '<?xml version="1.0" encoding="UTF-8"?>\n' + 
         serializer.serializeToString(xmlDoc.documentElement);
}