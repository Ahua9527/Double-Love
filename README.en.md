# Double Love

<div align="center">

[![GitHub license](https://img.shields.io/github/license/Ahua9527/Double-Love)](https://github.com/Ahua9527/Double-Love/blob/main/LICENSE)
![GitHub stars](https://img.shields.io/github/stars/Ahua9527/Double-Love)

<h3>🎬 Love Framed, Efficiency Amplified.</h3>

[//]: # (藏在代码里的彩蛋)
<!Double Love：让每个镜头都藏着我未说出口的帧率 -->

English · [简体中文](./README.md) · [Live Demo](https://double-love.ahua.space)

</div>

Double Love is an intelligent XML processing tool designed for film production, automating and standardizing script supervisor metadata.

## ✨ Features

- 🎬 Film industry workflow support
- 📝 Intelligent metadata standardization
- ⚡ Zero-latency local processing
- 🧩 Seamless Adobe Premiere integration

## 🚀 Quick Start

### Basic Workflow

1. Script Supervision: Generate interactive logging sheets using DTG Slate
2. Data Management: Import logging data through Silverstack Lab
3. File Export: Generate Adobe Premiere Pro XML
4. Standardization: Process with Double Love for intelligent optimization

Example:
- Input xml: `UnitA_304_20250127.xml`
- Output xml: `UnitA_304_20250127_Double_LOVE.xml`

### Detailed Examples

#### File Naming Optimization
- Automatic formatting of scene, shot, and take numbers
- Auto-padding numbers with leading zeros
- Automatic case standardization
- Redundant underscore cleanup

#### Clip Naming Convention

The processed clip names follow this format:
```
{project_prefix}{scene}_{shot}_{take}{camera}{rating}
```

- `prefix`: Custom prefix (optional)
- `scene`: Scene number (3 digits, e.g., 001)
- `shot`: Shot number (2 digits, e.g., 01)
- `take`: Take number (2 digits, e.g., 01)
- `camera`: Camera identifier (lowercase letter, e.g., a)
- `Rating`: Rating (ok/kp/ng)

#### Rating Processing
- `Circle` → `ok`
- `KEEP` → `kp`
- `NG` → `ng`

#### DIT Information
- Automatically adds DIT info: 'DIT: 哆啦Ahua 🌱'
- Deploy your own instance to modify 😁😁

#### Custom Prefix Examples

1. With prefix "PROJECT_A_":
   - Input file: `A304C007_250123G3`
   - Output file: `PROJECT_A_004_01_07a_kp`

2. Without prefix:
   - Input file: `A304C007_250123G3`
   - Output file: `004_01_07a_kp`

#### Sequence Resolution Examples

1. FHD Resolution (Default):
   - Width: 1920
   - Height: 1080

2. DCI 2K Resolution (Custom):
   - Width: 2048
   - Height: 1080

#### Batch Processing Example

1. Upload multiple files:
   ```
   UnitA_304_20250123.xml
   UnitA_305_20250124.xml
   UnitA_306_20250125.xml
   ```

2. Output files:
   ```
   UnitA_304_20250123_Double_LOVE.xml
   UnitA_305_20250124_Double_LOVE.xml
   UnitA_306_20250125_Double_LOVE.xml
   ```

## 🛠️ Tech Stack

- React 18
- TypeScript
- Vite
- Tailwind CSS
- Lucide Icons
- PWA Support

## 📦 Installation

1. Clone the repository

```bash
git clone https://github.com/Ahua9527/Double-Love.git
cd Double-Love
```

2. Install dependencies

```bash
npm install
```

3. Development

```bash
npm run dev
```

4. Build

```bash
npm run build
```

## 🔒 Security Notes

- All file processing occurs locally in the browser
- Maximum file size: 50MB
- Supports XML files only

## 🌈 Developer Guide

### Project Structure

```
Double-Love/
├── src/
│   ├── components/     # React components
│   ├── context/       # React Context
│   ├── utils/         # Utility functions
│   ├── styles/        # Style files
│   └── App.tsx        # Main application component
├── public/            # Static assets
└── ...config files
```

## 📃 License

[MIT License](LICENSE)

## 🤝 Contributing

Issues and Pull Requests are welcome!

## 👨‍💻 Author

哆啦Ahua🌱