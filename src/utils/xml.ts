// 类型定义
interface XMLProcessConfig {
  width?: number;      // 输出分辨率宽度
  height?: number;     // 输出分辨率高度
  format?: string;     // 文件名格式模板
  prefix?: string;     // 文件名前缀
}

// XML处理错误类型枚举
enum XMLProcessErrorType {
  INVALID_XML = 'INVALID_XML',                           // XML格式无效
  MISSING_REQUIRED_ELEMENTS = 'MISSING_REQUIRED_ELEMENTS', // 缺少必需元素
  INVALID_FORMAT = 'INVALID_FORMAT'                      // 格式错误
}

// 自定义XML处理错误类
class XMLProcessError extends Error {
  constructor(
    public type: XMLProcessErrorType,
    message: string
  ) {
    super(message);
    this.name = 'XMLProcessError';
  }
}

// clip元素相关接口
interface ClipElements {
  logginginfo: Element;      // 日志信息元素
  scene: Element;            // 场景元素
  shottake: Element;         // 镜头和场次元素
  filmdata: Element;         // 影片数据元素
  comments: Element;         // 评论元素
  mastercomment2: Element;   // 主评论2元素
}

// 处理后的clip数据接口
interface ProcessedClipData {
  sceneFormatted: string;    // 格式化后的场景号
  shotFormatted: string;     // 格式化后的镜头号
  takeFormatted: string;     // 格式化后的场次号
  cameraId: string;          // 摄影机标识
  rating: string;            // 评分(ok/kp/ng)
}

// 默认配置常量
const DEFAULT_CONFIG = {
  width: 1920,              // 默认宽度
  height: 1080,             // 默认高度
  format: '{scene}_{shot}_{take}{camera}{Rating}', // 默认文件名格式
  prefix: ''                // 默认前缀为空
} as const;

/**
 * 验证值是否有效
 * @param value - 待验证的字符串
 * @returns 布尔值表示是否有效
 */
function isValidValue(value: string): boolean {
  value = value.trim();
  if (!value) return false;
  if (/^-+$/.test(value)) return false;
  if (/^\s*-\s*$/.test(value)) return false;
  return true;
}

/**
 * 格式化场景号（保证3位数字）
 * @param scene - 原始场景号
 * @returns 格式化后的场景号
 */
function formatSceneNumber(scene: string): string {
  return scene.replace(/(\d+)/g, match => match.padStart(3, '0')).toUpperCase();
}

/**
 * 格式化镜头号和场次号（各保证2位数字）
 * @param value - 原始镜头和场次值
 * @returns [格式化的镜头号, 格式化的场次号]
 */
function formatShotTake(value: string): [string, string] {
  const [shot, take] = value.replace(/\s/g, '').split('-');
  return [
    shot.replace(/(\d+)/g, match => match.padStart(2, '0')).toLowerCase(),
    take.replace(/(\d+)/g, match => match.padStart(2, '0')).toLowerCase()
  ];
}

/**
 * 清理文件名（去除多余下划线）
 * @param name - 原始文件名
 * @returns 清理后的文件名
 */
function cleanupFileName(name: string): string {
  return name
    .replace(/_{2,}/g, '_')    // 替换多个连续下划线为单个
    .replace(/_+$/, '');       // 移除末尾下划线
}

/**
 * 获取评分值
 * @param comment - 评论内容
 * @returns 评分值(ok/kp/ng)
 */
function getRatingValue(comment: string): string {
  if (comment.includes("Circle")) return "ok";
  if (comment.includes("KEEP")) return "kp";
  if (comment.includes("NG")) return "ng";
  return "";
}

/**
 * 提取摄影机标识（取摄影机编号前1-2个字母）
 * @param camerarollText - 摄影机编号
 * @returns 摄影机标识(小写)
 */
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

/**
 * 提取clip中的必要元素
 * @param clip - clip元素
 * @returns 包含必要元素的对象或null
 */
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

/**
 * 处理clip数据
 * @param elements - clip元素集合
 * @returns 处理后的数据或null
 */
function processClipData(elements: ClipElements): ProcessedClipData | null {
  const { scene, shottake, filmdata, mastercomment2 } = elements;
  
  const sceneValue = scene.textContent || "";
  const shottakeValue = shottake.textContent || "";
  
  // 验证数据有效性
  if (!isValidValue(sceneValue) || !isValidValue(shottakeValue)) {
    return null;
  }
  
  // 检查shottake格式是否有效
  const shotTakeParts = shottakeValue.split('-');
  if (shotTakeParts.length !== 2 || 
      !shotTakeParts[0].trim() || 
      !shotTakeParts[1].trim()) {
    return null;
  }
  
  // 格式化数据
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

/**
 * 生成新文件名
 * @param data - 处理后的clip数据
 * @param config - 配置信息
 * @returns 新文件名
 */
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

/**
 * 更新相关XML元素
 * @param clip - 当前clip元素
 * @param xmlDoc - XML文档
 * @param newName - 新文件名
 */
function updateRelatedElements(clip: Element, xmlDoc: Document, newName: string): void {
  const clipId = clip.getAttribute('id');
  if (!clipId) return;
  
  // 更新clip名称
  const nameElem = clip.querySelector('name');
  if (nameElem) {
    nameElem.textContent = newName;
  }
  
  // 更新sequence相关元素
  const sequenceElem = xmlDoc.querySelector(`sequence[id="sequence_id_${clipId}"]`) ||
                      xmlDoc.querySelector(`sequence[id="sequence_${clipId}_ci"]`);
  
  if (sequenceElem) {
    // 更新sequence名称
    const sequenceName = sequenceElem.querySelector('name');
    if (sequenceName) {
      sequenceName.textContent = newName;
    }
    
    // 更新第三个name元素
    const nameElements = sequenceElem.getElementsByTagName('name');
    if (nameElements.length >= 3) {
      nameElements[2].textContent = newName;
    }
    
    // 更新clipitem名称
    const clipitem = sequenceElem.querySelector(`clipitem[id="sequence_${clipId}_ci"]`);
    if (clipitem) {
      const clipitemName = clipitem.querySelector('name');
      if (clipitemName) {
        clipitemName.textContent = newName;
      }
    }
  }
}

/**
 * 更新分辨率设置
 * @param xmlDoc - XML文档
 * @param config - 配置信息
 */
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

/**
 * 更新DIT信息
 * @param xmlDoc - XML文档
 */
function updateDITInfo(xmlDoc: Document): void {
  const lognoteElems = xmlDoc.getElementsByTagName('lognote');
  Array.from(lognoteElems).forEach(elem => {
    if (elem.textContent === 'DIT: (null)') {
      elem.textContent = 'DIT: 哆啦Ahua 🌱';
    }
  });
}

/**
 * XML处理主函数
 * @param file - XML文件
 * @param config - 配置信息
 * @returns 处理后的XML字符串
 */
export async function processXML(file: File, config?: XMLProcessConfig): Promise<string> {
  // 合并配置
  const finalConfig = { ...DEFAULT_CONFIG, ...config };
  
  // 解析XML
  const text = await file.text();
  const parser = new DOMParser();
  const xmlDoc = parser.parseFromString(text, 'text/xml');
  
  // 验证XML有效性
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
      
      // 处理clip数据
      const processedData = processClipData(elements);
      if (!processedData) continue;
      
      // 生成新文件名
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
