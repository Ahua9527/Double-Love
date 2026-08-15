/**
 * XML处理配置接口
 * 
 * @property {number} [width] - 输出视频宽度，默认1920
 * @property {number} [height] - 输出视频高度，默认1080
 * @property {string} [format] - 文件名格式模板，支持{scene}、{shot}等占位符
 * @property {Function} [onProgress] - 进度回调函数
 */
export interface XMLProcessConfig {
  width?: number;        
  height?: number;       
  format?: string;       
       
  onProgress?: (percent: number) => void;
  csvEpisodeMap?: Map<string, string>; // 可选的Episode映射
  csvSeasonMap?: Map<string, string>; // 可选的Season映射
}

export type XMLProcessStatus = 'success' | 'partial' | 'failed';
export type XMLDiagnosticLevel = 'error' | 'warning' | 'info';

export interface XMLDiagnostic {
  level: XMLDiagnosticLevel;
  code: string;
  message: string;
  clipId?: string;
  blocksDownload?: boolean;
}

export interface XMLProcessCounts {
  total: number;
  processed: number;
  ignored: number;
  skipped: number;
  failed: number;
  csvUnmatched: number;
}

export interface XMLProcessResult {
  status: XMLProcessStatus;
  counts: XMLProcessCounts;
  diagnostics: XMLDiagnostic[];
  xml?: string;
}

export interface CSVParseResult {
  seasonMap: Map<string, string>;
  episodeMap: Map<string, string>;
  diagnostics: XMLDiagnostic[];
}

/**
 * XML元素集合接口
 * 
 * @interface
 * @property {Element} logginginfo - 日志信息父元素
 * @property {Element} scene - 场景编号元素
 * @property {Element} shottake - 镜头拍摄元素
 * @property {Element} filmdata - 胶片数据元素
 * @property {Element} comments - 评论父元素
 * @property {Element} mastercomment2 - 主评论元素
 * @property {Element | null} labels - 标签元素
 */
interface ClipElements {
  logginginfo: Element;        
  scene: Element;              
  shottake: Element;           
  filmdata: Element;           
  comments: Element;           
  mastercomment2: Element;
  labels: Element | null;   // 修改为Element | null类型
}

/**
 * 处理后的剪辑数据接口
 * 
 * @interface
 * @property {string} sceneFormatted - 格式化后的场景值（数字段至少3位，兼容非纯数字场景值）
 * @property {string} shotFormatted - 格式化后的镜头段（Unicode字母/数字，数字段补到2位）
 * @property {string} takeFormatted - 格式化后的拍次段（Unicode字母/数字，数字段补到2位）
 * @property {string} cameraId - 摄影机标识符（2位字母）
 * @property {string} rating - 拍摄评级（ok/kp/ng）
 */
interface ProcessedClipData {
  sceneFormatted: string;      
  shotFormatted: string;       
  takeFormatted: string;       
  cameraId: string;            
  rating: string;            
}

/**
 * 默认配置常量
 * 
 * @constant
 * @type {Required<XMLProcessConfig>}
 */
const DEFAULT_CONFIG = {
  width: 1920,                
  height: 1080,               
  format: '{scene}_{shot}_{take}{camera}{Rating}',   
                 
  onProgress: () => {}
} as const;

/**
 * 验证输入值有效性
 * 
 * @param {string} value - 需要验证的字符串
 * @returns {boolean} 是否有效
 * 
 * 有效性规则：
 * 1. 不能为空或纯空格
 * 2. 不能全为连字符
 * 3. 不能是" - "格式
 */
type ClipDataValidationCode = 'INVALID_SCENE' | 'INVALID_SHOT_TAKE' | 'INVALID_CAMERA_ROLL';

interface ClipDataValidationResult {
  data: ProcessedClipData | null;
  code?: ClipDataValidationCode;
}

function isValidSceneValue(value: string): boolean {
  const normalized = value.trim();
  return Boolean(normalized) && !/^-+$/.test(normalized);
}

function splitShotTake(value: string): [string, string] | null {
  const normalized = value.replace(/\s/g, '');
  const parts = normalized.split('-');
  if (parts.length !== 2 || parts.some(part => !part)) return null;
  if (!parts.every(part => /^[\p{L}\p{N}]+$/u.test(part))) return null;
  if (!parts.every(part => /\p{Nd}/u.test(part))) return null;
  return [parts[0], parts[1]];
}

/**
 * 格式化场景编号
 * 
 * @param {string} scene - 原始场景编号
 * @returns {string} 格式化后的场景值（数字段至少3位，拉丁字母大写）
 * 
 * @example
 * formatSceneNumber("A12") => "A012"
 */
function formatSceneNumber(scene: string): string {
  return scene.replace(/(\d+)/g, match => match.padStart(3, '0')).toUpperCase();
}

/**
 * 格式化镜头和拍摄编号
 * 
 * @param {string} value - 原始镜头拍摄字符串（格式：镜头-拍摄）
 * @returns {[string, string]} 格式化后的镜头/拍次段（各段数字补到2位，拉丁字母小写）
 * 
 * @example
 * formatShotTake("3-5") => ["03", "05"]
 */
function formatShotTake(value: string): [string, string] {
  const [shot, take] = splitShotTake(value)!;
  return [
    shot
      .replace(/\p{Decimal_Number}+/gu, match => match.padStart(2, '0'))
      .replace(/[A-Z]/g, letter => letter.toLowerCase()),
    take
      .replace(/\p{Decimal_Number}+/gu, match => match.padStart(2, '0'))
      .replace(/[A-Z]/g, letter => letter.toLowerCase())
  ];
}

/**
 * 清理文件名中的无效字符
 * 
 * @param {string} name - 原始文件名
 * @returns {string} 清理后的文件名
 * 
 * 清理规则：
 * 1. 替换连续下划线为单个
 * 2. 去除末尾下划线
 */
function cleanupFileName(name: string): string {
  return name
    .replace(/_{2,}/g, '_')        
    .replace(/_+$/, '');       
}

/**
 * 从标签中提取拍摄评级
 * @param {Element | null} labelsElement - 标签元素
 * @returns {string} 评级标识（小写形式）
 * 
 * 提取规则：
 * - 如果包含"No Label"则不提取（返回空字符串）
 * - 特殊处理: "keep" 或 "kp" 统一返回 "kp"
 * - 其他任何内容都提取并转换为小写
 */
export function getRatingFromLabels(labelsElement: Element | null): string {
  // 如果标签元素不存在，返回空字符串
  if (!labelsElement) return "";

  // 获取label元素的文本内容
  const labelElem = labelsElement.querySelector('label');
  const labelText = labelElem?.textContent?.trim() || "";
  
  // 转换为小写进行比较
  const lowerLabelText = labelText.toLowerCase();
  
  // 如果包含"no label"，则不提取
  if (lowerLabelText.includes("no label")) {
    return ""; // 不识别
  }
  
  // 特殊处理: "keep" 或 "kp" 统一返回 "kp"
  if (lowerLabelText.includes("keep") || lowerLabelText.includes("kp")) {
    return "kp";
  }
  
  // 特殊处理: "circle" 统一返回 "ok"（对齐文档声明的评级映射）
  if (lowerLabelText === "circle") {
    return "ok";
  }

  // 其他任何内容都提取并转换为小写
  return lowerLabelText;
}

/**
 * 获取摄影机标识符
 * 
 * @param {string} camerarollText - 摄影机卷号文本
 * @returns {string} 2位小写字母标识
 * 
 * @example
 * getCameraIdentifier("A001") => "a"
 * getCameraIdentifier("BCam002") => "bc"
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
 * 从clip元素中提取必要子元素
 * 
 * @param {Element} clip - XML clip元素
 * @returns {ClipElements | null} 提取到的元素集合或null
 * 
 * 需要提取的元素包括：
 * - logginginfo
 * - scene
 * - shottake
 * - filmdata
 * - comments
 * - mastercomment2
 * - labels
 */
function extractClipElements(clip: Element): ClipElements | null {
  const logginginfo = clip.querySelector('logginginfo');
  const scene = logginginfo?.querySelector('scene');
  const shottake = logginginfo?.querySelector('shottake');
  const filmdata = clip.querySelector('filmdata');
  const comments = clip.querySelector('comments');
  const mastercomment2 = comments?.querySelector('mastercomment2');
  const labels = clip.querySelector('labels'); // 直接从clip中提取labels
  
  if (!logginginfo || !scene || !shottake || !filmdata || !comments || !mastercomment2) {
    return null;
  }
  
  return { logginginfo, scene, shottake, filmdata, comments, mastercomment2, labels };
}

/**
 * 处理剪辑数据
 * 
 * @param {ClipElements} elements - 提取到的元素集合
 * @returns {ProcessedClipData | null} 处理后的数据或null
 * 
 * 处理流程：
 * 1. 验证场景和镜头数据有效性
 * 2. 格式化场景、镜头、拍摄编号
 * 3. 提取摄影机标识
 * 4. 从labels元素提取拍摄评级
 */
function validateClipData(elements: ClipElements): ClipDataValidationResult {
  const { scene, shottake, filmdata, labels } = elements;
  
  const sceneValue = scene.textContent || "";
  const shottakeValue = shottake.textContent || "";
  
  if (!isValidSceneValue(sceneValue)) {
    return { data: null, code: 'INVALID_SCENE' };
  }
  
  if (!splitShotTake(shottakeValue)) {
    return { data: null, code: 'INVALID_SHOT_TAKE' };
  }
  
  // 格式化数据
  const sceneFormatted = formatSceneNumber(sceneValue);
  const [shotFormatted, takeFormatted] = formatShotTake(shottakeValue);
  
  // 提取摄影机标识
  const cameraroll = filmdata.querySelector('cameraroll');
  if (!cameraroll?.textContent) {
    return { data: null, code: 'INVALID_CAMERA_ROLL' };
  }
  
  const cameraId = getCameraIdentifier(cameraroll.textContent);
  if (!cameraId) {
    return { data: null, code: 'INVALID_CAMERA_ROLL' };
  }
  
  // 提取评级信息 - 使用提取的labels元素
  const rating = getRatingFromLabels(labels);
  
  return {
    data: {
      sceneFormatted,
      shotFormatted,
      takeFormatted,
      cameraId,
      rating
    },
  };
}

export function processClipData(elements: ClipElements): ProcessedClipData | null {
  return validateClipData(elements).data;
}

/**
 * 将 CSV Name、XML clip id 和文件名转换为同一个匹配键。
 * 匹配只使用 basename，并统一大小写和常见媒体扩展名。
 */
export function normalizeMatchKey(value: string): string {
  const basename = value
    .replace(/^\uFEFF/, '')
    .trim()
    .split(/[\\/]/)
    .pop() || '';

  return basename
    .replace(/\.(mxf|mov|mp4|m4v|wav|bwf|aif|aiff|ari|arx|dng|r3d|braw|crm|xml|ale|bin)$/i, '')
    .trim()
    .toLocaleLowerCase();
}

function parseCSVRows(csvContent: string): { rows: string[][]; error?: string } {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = '';
  let quoted = false;
  const content = csvContent.replace(/^\uFEFF/, '');

  for (let index = 0; index < content.length; index += 1) {
    const character = content[index];

    if (character === '"') {
      if (quoted && content[index + 1] === '"') {
        field += '"';
        index += 1;
      } else {
        quoted = !quoted;
      }
      continue;
    }

    if (character === ',' && !quoted) {
      row.push(field);
      field = '';
      continue;
    }

    if ((character === '\n' || character === '\r') && !quoted) {
      row.push(field);
      field = '';
      if (row.some(value => value.trim() !== '')) {
        rows.push(row);
      }
      row = [];
      if (character === '\r' && content[index + 1] === '\n') {
        index += 1;
      }
      continue;
    }

    field += character;
  }

  if (quoted) {
    return { rows, error: 'CSV_QUOTED_FIELD_UNTERMINATED' };
  }

  if (field !== '' || row.length > 0) {
    row.push(field);
    if (row.some(value => value.trim() !== '')) {
      rows.push(row);
    }
  }

  return { rows };
}

function normalizeHeader(value: string): string {
  return value.replace(/^\uFEFF/, '').trim().toLocaleLowerCase();
}

function normalizePositiveCode(value: string): string | null {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) return null;

  const numericValue = Number(trimmed);
  if (!Number.isSafeInteger(numericValue) || numericValue < 1 || numericValue > 99) return null;

  return String(numericValue).padStart(2, '0');
}

/**
 * 解析 CSV 文件获取 Season 和 Episode 映射。
 * 支持引号、字段内逗号、BOM 和 CRLF；非法编号会被拒绝并记录诊断。
 */
export function parseCSVForSeasonEpisode(csvContent: string): CSVParseResult {
  const seasonMap = new Map<string, string>();
  const episodeMap = new Map<string, string>();
  const diagnostics: XMLDiagnostic[] = [];
  const parsed = parseCSVRows(csvContent);

  if (parsed.error) {
    diagnostics.push({
      level: 'error',
      code: parsed.error,
      message: 'CSV 存在未闭合的引号，无法安全解析。',
      blocksDownload: true,
    });
    return { seasonMap, episodeMap, diagnostics };
  }

  if (parsed.rows.length === 0) {
    diagnostics.push({
      level: 'warning',
      code: 'CSV_EMPTY',
      message: 'CSV 为空，没有可用于匹配的记录。',
    });
    return { seasonMap, episodeMap, diagnostics };
  }

  const headers = parsed.rows[0].map(normalizeHeader);
  const seenHeaders = new Set<string>();
  const duplicateHeader = headers.find(header => {
    if (!header) return false;
    if (seenHeaders.has(header)) return true;
    seenHeaders.add(header);
    return false;
  });

  if (duplicateHeader) {
    diagnostics.push({
      level: 'error',
      code: 'CSV_HEADER_DUPLICATE',
      message: `CSV 表头 ${duplicateHeader} 重复，无法确定应使用哪一列。`,
      blocksDownload: true,
    });
    return { seasonMap, episodeMap, diagnostics };
  }

  const nameIndex = headers.findIndex(header => header === 'name');
  const seasonIndex = headers.findIndex(header => header === 'season');
  const episodeIndex = headers.findIndex(header => header === 'episode');

  if (nameIndex === -1) {
    diagnostics.push({
      level: 'error',
      code: 'CSV_NAME_COLUMN_MISSING',
      message: 'CSV 缺少 Name 列，无法与 XML clip 匹配。',
      blocksDownload: true,
    });
    return { seasonMap, episodeMap, diagnostics };
  }

  for (let rowIndex = 1; rowIndex < parsed.rows.length; rowIndex += 1) {
    if (parsed.rows[rowIndex].length !== headers.length) {
      diagnostics.push({
        level: 'error',
        code: 'CSV_COLUMN_COUNT_MISMATCH',
        message: `CSV 第 ${rowIndex + 1} 行有 ${parsed.rows[rowIndex].length} 列，表头有 ${headers.length} 列。`,
        blocksDownload: true,
      });
      return { seasonMap, episodeMap, diagnostics };
    }
  }

  if (seasonIndex === -1 && episodeIndex === -1) {
    diagnostics.push({
      level: 'warning',
      code: 'CSV_METADATA_COLUMNS_MISSING',
      message: 'CSV 只有 Name 列，没有 Season 或 Episode 数据。',
    });
  }

  for (let rowIndex = 1; rowIndex < parsed.rows.length; rowIndex += 1) {
    const columns = parsed.rows[rowIndex];
    const line = rowIndex + 1;
    const fileName = columns[nameIndex]?.trim() || '';
    const matchKey = normalizeMatchKey(fileName);

    if (!matchKey) {
      diagnostics.push({
        level: 'warning',
        code: 'CSV_NAME_EMPTY',
        message: `第 ${line} 行缺少 Name，已跳过。`,
      });
      continue;
    }

    if (seasonIndex !== -1) {
      const rawSeason = columns[seasonIndex]?.trim() || '';
      if (!rawSeason) {
        diagnostics.push({
          level: 'warning',
          code: 'CSV_SEASON_EMPTY',
          message: `第 ${line} 行 Season 为空，已忽略该值。`,
        });
      } else {
        const season = normalizePositiveCode(rawSeason);
        if (!season) {
          diagnostics.push({
            level: 'error',
            code: 'CSV_SEASON_INVALID',
            message: `第 ${line} 行 Season 必须是 1 到 99 的十进制整数。`,
            blocksDownload: true,
          });
        } else {
          if (seasonMap.has(matchKey)) {
            diagnostics.push({
              level: 'warning',
              code: 'CSV_SEASON_DUPLICATE',
              message: `Name ${fileName} 存在重复 Season，已采用最后一条记录。`,
            });
          }
          seasonMap.set(matchKey, season);
        }
      }
    }

    if (episodeIndex !== -1) {
      const rawEpisode = columns[episodeIndex]?.trim() || '';
      if (!rawEpisode) {
        diagnostics.push({
          level: 'warning',
          code: 'CSV_EPISODE_EMPTY',
          message: `第 ${line} 行 Episode 为空，已忽略该值。`,
        });
      } else {
        const episode = normalizePositiveCode(rawEpisode);
        if (!episode) {
          diagnostics.push({
            level: 'error',
            code: 'CSV_EPISODE_INVALID',
            message: `第 ${line} 行 Episode 必须是 1 到 99 的十进制整数。`,
            blocksDownload: true,
          });
        } else {
          if (episodeMap.has(matchKey)) {
            diagnostics.push({
              level: 'warning',
              code: 'CSV_EPISODE_DUPLICATE',
              message: `Name ${fileName} 存在重复 Episode，已采用最后一条记录。`,
            });
          }
          episodeMap.set(matchKey, episode);
        }
      }
    }
  }

  return { seasonMap, episodeMap, diagnostics };
}

/**
 * 生成新文件名
 * 
 * @param {ProcessedClipData} data - 处理后的剪辑数据
 * @param {XMLProcessConfig} config - 配置参数
 * @param {string} originalFileName - 原始文件名
 * @returns {string} 生成的文件名
 * 
 * 替换规则：
 * {season} -> 季数编号（如果存在）
 * {episode} -> 集数编号（如果存在）
 * {scene} -> 场景编号
 * {shot} -> 镜头编号
 * {take} -> 拍摄编号
 * {camera} -> 摄影机标识
 * {Rating} -> 评级后缀（带下划线）
 */
export function generateNewName(data: ProcessedClipData, config: XMLProcessConfig, originalFileName: string): string {
  // 检查是否有Season和Episode数据
  const matchKey = normalizeMatchKey(originalFileName);
  const season = config.csvSeasonMap?.get(matchKey) || config.csvSeasonMap?.get(originalFileName);
  const episode = config.csvEpisodeMap?.get(matchKey) || config.csvEpisodeMap?.get(originalFileName);
  
  // 动态选择命名格式
  let format = config.format || DEFAULT_CONFIG.format;
  
  // 根据数据可用性动态构建格式
  if (season && episode) {
    // 有Season和Episode时
    if (!format.startsWith('{season}')) {
      format = '{season}_{episode}_' + format;
    }
  } else if (episode) {
    // 仅有Episode时
    if (!format.startsWith('{episode}')) {
      format = '{episode}_' + format;
    }
  }
  
  let newName = format
    .replace('{season}', season || '') // 如果没有season，替换为空
    .replace('{episode}', episode || '') // 如果没有episode，替换为空
    .replace('{scene}', data.sceneFormatted)
    .replace('{shot}', data.shotFormatted)
    .replace('{take}', data.takeFormatted)
    .replace('{camera}', data.cameraId)
    .replace('{Rating}', data.rating ? `_${data.rating}` : '');
  
  // 清理开头的下划线（当没有season/episode时可能出现）
  newName = newName.replace(/^_+/, '');
  
  newName = cleanupFileName(newName);
  
  return newName;
}

/**
 * 处理标签元素，修正拼写错误
 * 
 * @param {Element | null} labelsElement - 标签元素
 * @returns {void} 
 */
function fixLabelsSpelling(labelsElement: Element | null): void {
  if (!labelsElement) return;
  
  // 查找label2元素
  const label2Elem = labelsElement.querySelector('label2');
  if (!label2Elem) return;
  
  // 获取label2的文本内容
  const label2Text = label2Elem.textContent || "";
  
  // 如果包含"Celurean"，修正为"Cerulean"
  if (label2Text.includes("Celurean")) {
    label2Elem.textContent = label2Text.replace("Celurean", "Cerulean");
  }
}

/**
 * 更新相关XML元素
 * 
 * @param {Element} clip - 当前clip元素
 * @param {Document} xmlDoc - XML文档对象
 * @param {string} newName - 新文件名
 * 
 * 更新范围包括：
 * 1. 当前clip的name元素
 * 2. 关联sequence元素的name元素
 * 3. 关联clipitem的name元素
 * 4. 将clip的labels元素复制到sequence元素和其中的clipitem元素
 * 5. 修正label2中Celurean的拼写
 */
function updateRelatedElements(clip: Element, xmlDoc: Document, newName: string): void {
  const clipId = clip.getAttribute('id');
  if (!clipId) return;

  // 外部 XML 的 id 可能包含 CSS 选择器特殊字符。通过属性值精确比较，
  // 并在开始写回前先完成目标查找，避免留下半修改结果。
  const sequenceElem = Array.from(xmlDoc.getElementsByTagName('sequence')).find(sequence => (
    sequence.getAttribute('id') === `sequence_id_${clipId}` ||
    sequence.getAttribute('id') === `sequence_${clipId}_ci`
  ));
  
  // 获取clip中的labels元素
  const labelsElem = clip.querySelector('labels');
  
  // 修正labels元素中的拼写错误
  fixLabelsSpelling(labelsElem);
  
  // 更新clip的name元素
  const nameElem = clip.querySelector('name');
  if (nameElem) {
    nameElem.textContent = newName;
  }
  
  if (sequenceElem) {
    // 更新sequence的name元素
    const sequenceName = sequenceElem.querySelector('name');
    if (sequenceName) {
      sequenceName.textContent = newName;
    }
    
    // 更新sequence中的clipitem相关元素
    updateClipItems(sequenceElem, newName, labelsElem);
    
    // 复制labels元素到sequence元素末尾
    if (labelsElem) {
      // 检查sequence是否已有labels元素
      const sequenceLabelsElem = sequenceElem.querySelector(':scope > labels');
      if (sequenceLabelsElem) {
        // 如果存在，则替换内容
        sequenceLabelsElem.innerHTML = labelsElem.innerHTML;
      } else {
        // 如果不存在，则复制并添加到sequence元素的末尾
        const clonedLabels = labelsElem.cloneNode(true);
        sequenceElem.appendChild(clonedLabels);
      }
    }
  }
}

/**
 * 更新sequence中的clipitem元素
 * 
 * @param {Element} sequenceElem - sequence元素
 * @param {string} newName - 新文件名
 * @param {Element | null} labelsElem - 标签元素
 */
function updateClipItems(sequenceElem: Element, newName: string, labelsElem: Element | null): void {
  // 更新所有video track中的clipitem
  const videoTrackClipitems = sequenceElem.querySelectorAll('video > track > clipitem');
  for (const clipitem of Array.from(videoTrackClipitems)) {
    const clipitemName = clipitem.querySelector('name');
    if (clipitemName) {
      clipitemName.textContent = newName;
    }
    
    // 复制labels元素到video clipitem
    copyLabelsToElement(clipitem, labelsElem);
  }
  
  // 更新所有audio track中的clipitem
  const audioTrackClipitems = sequenceElem.querySelectorAll('audio > track > clipitem');
  for (const clipitem of Array.from(audioTrackClipitems)) {
    // 复制labels元素到audio clipitem
    copyLabelsToElement(clipitem, labelsElem);
  }
}

/**
 * 复制labels元素到目标元素
 * 
 * @param {Element} targetElem - 目标元素
 * @param {Element | null} labelsElem - 标签元素
 */
function copyLabelsToElement(targetElem: Element, labelsElem: Element | null): void {
  if (!labelsElem) return;
  
  // 检查目标元素是否已有labels元素
  const targetLabelsElem = targetElem.querySelector('labels');
  if (targetLabelsElem) {
    // 如果存在，则替换内容
    targetLabelsElem.innerHTML = labelsElem.innerHTML;
  } else {
    // 如果不存在，则复制并添加到目标元素中
    const clonedLabels = labelsElem.cloneNode(true);
    targetElem.appendChild(clonedLabels);
  }
}

/**
 * 更新分辨率设置
 * 
 * @param {Document} xmlDoc - XML文档对象
 * @param {object} config - 配置参数
 * @param {number} config.width - 宽度
 * @param {number} config.height - 高度
 * 
 * 更新所有width和height元素的文本内容
 */
export function updateResolution(xmlDoc: Document, config: { width: number; height: number }): void {
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
 * 
 * @param {Document} xmlDoc - XML文档对象
 * 
 * 将所有"DIT: (null)"替换为"Generated by https://double-love.ahua.space"
 */
function updateDITInfo(xmlDoc: Document): void {
  const lognoteElems = xmlDoc.getElementsByTagName('lognote');
  Array.from(lognoteElems).forEach(elem => {
    if (elem.textContent === 'DIT: (null)') {
      elem.textContent = 'Generated by https://double-love.ahua.space';
    }
  });
}


/**
 * 处理路径URL函数
 * 
 * @param {Document} xmlDoc - XML文档对象
 * 
 * 使用正则表达式找到并替换pathurl元素中的序列帧文件名：
 * 1. 将匹配 \.\d+\.arx 的内容替换为 .arx
 * 2. 将匹配 \.\d+\.ari 的内容替换为 .ari
 * 3. 将匹配 _\d+\.dng 的内容替换为 .dng
 */
function processPathURLs(xmlDoc: Document): void {
  // 获取所有pathurl元素
  const pathurlElems = xmlDoc.getElementsByTagName('pathurl');
  
  // 遍历处理每个pathurl元素
  Array.from(pathurlElems).forEach(elem => {
    if (elem.textContent) {
      // 使用正则表达式替换 \.\d+\.arx 为 .arx
      let newValue = elem.textContent.replace(/\.\d+\.arx/g, '.arx');
      // 使用正则表达式替换 \.\d+\.ari 为 .ari
      newValue = newValue.replace(/\.\d+\.ari/g, '.ari');
      // 使用正则表达式替换 _\d+\.dng 为 .dng
      newValue = newValue.replace(/_\d+\.dng/g, '.dng');
      elem.textContent = newValue;
    }
  });
}



export function isStrictPositiveInteger(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value > 0;
}

function failedXMLResult(code: string, message: string): XMLProcessResult {
  return {
    status: 'failed',
    counts: {
      total: 0,
      processed: 0,
      ignored: 0,
      skipped: 0,
      failed: 0,
      csvUnmatched: 0,
    },
    diagnostics: [{ level: 'error', code, message, blocksDownload: true }],
  };
}

function getMappedValue(map: Map<string, string> | undefined, originalFileName: string): string | undefined {
  if (!map) return undefined;

  return map.get(normalizeMatchKey(originalFileName)) || map.get(originalFileName);
}

function getClipValidationMessage(code: ClipDataValidationCode): string {
  if (code === 'INVALID_SCENE') return 'clip 的场景号无效，已跳过。';
  if (code === 'INVALID_SHOT_TAKE') return 'clip 的镜头/拍次格式无效，已跳过。';
  return 'clip 的摄影机卷号无效，已跳过。';
}

function isPlaceholderValue(value: string): boolean {
  const normalized = value.trim().toLocaleLowerCase();
  return !normalized || /^-+$/.test(normalized) || ['n/a', 'na', 'null', '(null)', 'none'].includes(normalized);
}

function getIgnoredClipCode(clip: Element): 'IGNORED_AUDIO_ONLY' | 'IGNORED_STILL_IMAGE' | null {
  const media = Array.from(clip.children).find(child => child.tagName === 'media');
  const hasAudio = Boolean(media?.querySelector('audio'));
  const hasVideo = Boolean(media?.querySelector('video'));
  if (hasAudio && !hasVideo) return 'IGNORED_AUDIO_ONLY';

  const references = Array.from(clip.querySelectorAll('name, pathurl'))
    .map(element => element.textContent?.trim() || '');
  const referencesStillImage = references.some(reference => /\.(?:jpg|jpeg)$/i.test(reference));
  if (!referencesStillImage) return null;

  const scene = clip.querySelector('logginginfo scene')?.textContent || '';
  const shottake = clip.querySelector('logginginfo shottake')?.textContent || '';
  return isPlaceholderValue(scene) || isPlaceholderValue(shottake)
    ? 'IGNORED_STILL_IMAGE'
    : null;
}

function getIgnoredClipMessage(code: 'IGNORED_AUDIO_ONLY' | 'IGNORED_STILL_IMAGE'): string {
  return code === 'IGNORED_AUDIO_ONLY'
    ? '纯音频 clip 不参与命名和标签写回，节点保留在 XML。'
    : '缺少有效场景或镜头拍次的 JPEG 静帧不参与命名和标签写回，节点保留在 XML。';
}

/**
 * 处理 XML 文件并返回可审计的结构化结果。
 * 处理出至少一个合法 clip 时，即使存在跳过或失败，也会提供 partial XML，
 * 但调用方必须根据 status 和 diagnostics 向用户明确说明结果。
 */
export async function processXML(file: File, config?: XMLProcessConfig): Promise<XMLProcessResult> {
  const finalConfig = { ...DEFAULT_CONFIG, ...config };

  if (!isStrictPositiveInteger(finalConfig.width) || !isStrictPositiveInteger(finalConfig.height)) {
    return failedXMLResult('INVALID_RESOLUTION', '宽度和高度必须是正整数。');
  }

  let text: string;
  try {
    text = await file.text();
  } catch {
    return failedXMLResult('FILE_READ_FAILED', '无法读取 XML 文件。');
  }

  const parser = new DOMParser();
  const xmlDoc = parser.parseFromString(text, 'text/xml');

  if (xmlDoc.getElementsByTagName('parsererror').length > 0) {
    return failedXMLResult('INVALID_XML', '无效的 XML 文件，未生成下载结果。');
  }

  const diagnostics: XMLDiagnostic[] = [];

  const clips = Array.from(xmlDoc.getElementsByTagName('clip'));
  const total = clips.length;
  let processedCount = 0;
  let ignoredCount = 0;
  let skippedCount = 0;
  let failedCount = 0;
  let csvUnmatched = 0;
  const hasCsvMapping = Boolean(finalConfig.csvSeasonMap?.size || finalConfig.csvEpisodeMap?.size);

  if (total === 0) {
    return failedXMLResult('NO_CLIPS', 'XML 中没有可处理的 clip。');
  }

  for (let index = 0; index < clips.length; index += 1) {
    const clip = clips[index];
    const clipId = clip.getAttribute('id') || undefined;

    const ignoredCode = getIgnoredClipCode(clip);
    if (ignoredCode) {
      ignoredCount += 1;
      diagnostics.push({
        level: 'info',
        code: ignoredCode,
        message: getIgnoredClipMessage(ignoredCode),
        ...(clipId ? { clipId } : {}),
      });
      finalConfig.onProgress?.(Math.round(((index + 1) / total) * 100));
      continue;
    }

    if (!clipId) {
      skippedCount += 1;
      diagnostics.push({
        level: 'warning',
        code: 'MISSING_CLIP_ID',
        message: 'clip 缺少 id，无法可靠更新关联元素，已跳过。',
      });
      finalConfig.onProgress?.(Math.round(((index + 1) / total) * 100));
      continue;
    }

    try {
      const elements = extractClipElements(clip);
      if (!elements) {
        skippedCount += 1;
        diagnostics.push({
          level: 'warning',
          code: 'MISSING_CLIP_FIELDS',
          message: 'clip 缺少必要字段，已跳过。',
          clipId,
        });
      } else {
        const validation = validateClipData(elements);
        if (!validation.data) {
          skippedCount += 1;
          diagnostics.push({
            level: 'warning',
            code: validation.code || 'INVALID_SCENE',
            message: getClipValidationMessage(validation.code || 'INVALID_SCENE'),
            clipId,
          });
        } else {
          const originalFileName = clipId || clip.querySelector('name')?.textContent || '';
          const season = getMappedValue(finalConfig.csvSeasonMap, originalFileName);
          const episode = getMappedValue(finalConfig.csvEpisodeMap, originalFileName);

          if (hasCsvMapping && !season && !episode) {
            csvUnmatched += 1;
            diagnostics.push({
              level: 'warning',
              code: 'CSV_CLIP_UNMATCHED',
              message: 'clip 没有匹配到 CSV Name，已使用无 CSV 前缀的命名规则。',
              clipId,
            });
          }

          const newName = generateNewName(validation.data, finalConfig, originalFileName);
          updateRelatedElements(clip, xmlDoc, newName);
          processedCount += 1;
        }
      }
    } catch (error) {
      failedCount += 1;
      diagnostics.push({
        level: 'error',
        code: 'CLIP_PROCESSING_FAILED',
        message: error instanceof Error ? error.message : '处理 clip 时发生未知错误。',
        clipId,
      });
    }

    finalConfig.onProgress?.(Math.round(((index + 1) / total) * 100));
  }

  updateResolution(xmlDoc, {
    width: finalConfig.width,
    height: finalConfig.height,
  });
  updateDITInfo(xmlDoc);
  processPathURLs(xmlDoc);

  const counts: XMLProcessCounts = {
    total,
    processed: processedCount,
    ignored: ignoredCount,
    skipped: skippedCount,
    failed: failedCount,
    csvUnmatched,
  };

  if (processedCount === 0) {
    diagnostics.push({
      level: 'error',
      code: total > 0 && ignoredCount === total ? 'NO_PROCESSABLE_VIDEO_CLIPS' : 'NO_CLIPS_PROCESSED',
      message: total > 0 && ignoredCount === total
        ? 'XML 中没有可处理的视频 clip，未生成下载结果。'
        : '没有任何 clip 成功处理，未生成下载结果。',
      blocksDownload: true,
    });
  }

  const serializer = new XMLSerializer();
  const xml = processedCount > 0
    ? '<?xml version="1.0" encoding="UTF-8"?>\n' + serializer.serializeToString(xmlDoc.documentElement)
    : undefined;
  const hasProblems = skippedCount > 0 || failedCount > 0 || csvUnmatched > 0;

  return {
    status: processedCount === 0 ? 'failed' : hasProblems ? 'partial' : 'success',
    counts,
    diagnostics,
    ...(xml ? { xml } : {}),
  };
}
