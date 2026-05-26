# Dawn Smart Glasses Schematic Draft

版本日期：2026-04-21
状态：系统级原理图草案，可评审，不可直接量产投板

> Page 1 和 Page 2 的生产意图版本已经单独落地，优先使用以下文件，而不是本草案中的旧 `PCA9420` 供电描述：
>
> - [dawn_smart_glasses_page1_power_production_intent.md](D:/Agent2Agent应用/docs/hardware/dawn_smart_glasses_page1_power_production_intent.md)
> - [dawn_smart_glasses_page2_rt685_production_intent.md](D:/Agent2Agent应用/docs/hardware/dawn_smart_glasses_page2_rt685_production_intent.md)
> - [dawn_smart_glasses_page1_power_production_intent.svg](D:/Agent2Agent应用/docs/hardware/dawn_smart_glasses_page1_power_production_intent.svg)
> - [dawn_smart_glasses_page2_rt685_production_intent.svg](D:/Agent2Agent应用/docs/hardware/dawn_smart_glasses_page2_rt685_production_intent.svg)
> - [dawn_smart_glasses_page1_page2_production_bom.csv](D:/Agent2Agent应用/docs/hardware/dawn_smart_glasses_page1_page2_production_bom.csv)

## 设计目标

这套电路不是把完整 Dawn 和完整 Gemma4 直接塞进眼镜本体，而是把眼镜做成：

- 一个带近眼显示、相机、麦克风阵列、低功耗传感器和人在环确认能力的智能终端
- 由眼镜主控负责媒体链路、交互和无线连接
- 由低功耗 MCU 负责 always-on 语音、姿态/接近感知、物理确认和本地安全根
- 由伴随手机或边缘节点继续运行 Dawn / Gemma4 主运行时

这个方向和当前工作区里的 Dawn 能力是对齐的：

- [README](D:/Agent2Agent应用/README.md) 说明 Dawn 是本地优先 Agent Runtime，不是单模型 SDK
- [gemma4_ollama_integration.md](D:/Agent2Agent应用/docs/gemma4_ollama_integration.md) 说明 Gemma4 当前还是通过本地 Ollama 跑在伴随算力侧更稳
- [dawn_mcu/main.rs](D:/Agent2Agent应用/dawn_mcu/src/main.rs) 已经体现了“物理按键 + 本地签名”这类 MCU 侧能力，适合落成眼镜端安全确认子系统

## 文件

- 总图：[dawn_smart_glasses_system_schematic.svg](D:/Agent2Agent应用/docs/hardware/dawn_smart_glasses_system_schematic.svg)
- 电源树：[dawn_smart_glasses_power_tree.svg](D:/Agent2Agent应用/docs/hardware/dawn_smart_glasses_power_tree.svg)

## 核心器件

### 主控与控制

- `U100`: Qualcomm Snapdragon AR1 Gen 1 module / SOM
- `U200`: NXP `MIMXRT685SFAWBR`
- `U210`: NXP `PCA9420`
- `U220`: NXP `SE050`

### 传感器与音频

- `U300`: Bosch `BHI360`
- `U301`: Bosch `BMM350`
- `U310`: ams `TMF8828`
- `U311`: ams `TMD3725`
- `U320-U323`: TDK `T5838`

### 供电

- `U400`: TI `BQ25619`
- `U401`: NXP `NX5P3090UK`
- `U402`: ADI `MAX17048`
- `BT400`: EEMB `LP452030`

### 媒体链路

- `U500`: Sony `IMX662-AAQR1`
- `U510`: JBD `Hummingbird II` driver / engine
- `U511`: Sony `ECX350F` driver board
- `OPT510`: Cellid `Glass Waveguide G3`

## 原理图页 1：电源前端

### 电源链路

```text
J400 USB-C Receptacle (Sink-only, 5 V)
  CC1 -> R401 5.1k -> GND
  CC2 -> R402 5.1k -> GND
  VBUS -> D400 TPD4E05U06 -> U401 NX5P3090UK IN
  U401 OUT -> +VBUS_PROT_5V -> U400 BQ25619 VIN

U400 BQ25619
  BAT -> BT400 LP452030
  SYS -> +SYS_MAIN
  TS  -> NTC 10k battery thermistor network
  SDA/SCL -> I2C_PM
  INT -> RT685 GPIO

U402 MAX17048
  CELL -> BAT+
  GND  -> BAT-
  SDA/SCL -> U403 PCA9306 -> I2C_AON_1V8
  ALRT -> RT685 GPIO

+SYS_MAIN -> U100 AR1_MODULE VIN_MAIN
+SYS_MAIN -> U210 PCA9420 VIN
```

### 电源树建议

- `+SYS_MAIN`：由 `BQ25619 SYS` 输出，作为眼镜主系统主输入
- `+VDD_RT685_CORE_1V1`：`PCA9420 SW1`
- `+VDD_SENSOR_IO_1V8`：`PCA9420 SW2`
- `+VDD_AUX_3V3`：`PCA9420 LDO2`
- `+VDD_AON_1V8`：`PCA9420 LDO1`

### 关键设计说明

- 充电口先按 `5 V sink-only` 设计，避免在第一版 EVT 里引入 USB-PD 协议复杂度。
- 如果后续确定要支持快充或高功率外设，再把 `TUSB320` / `TPS25750` 一类 PD 控制器加回去。
- `MAX17048` 直接挂电池侧更合理，但因为电池侧电压高于 `1.8 V` 传感器总线，所以增加 `PCA9306` 进行 I2C 电平转换。

## 原理图页 2：RT685 低功耗控制与 AON 总线

### RT685 子系统

```text
U200 RT685
  VDDCORE   <- +VDD_RT685_CORE_1V1
  VDDIO     <- +VDD_SENSOR_IO_1V8
  RESETn    <- R210 10k pull-up to +VDD_AON_1V8
  ISP/UART0 -> J201 debug header
  SPI/QSPI  -> U230 external NOR flash
  I2C0      -> I2C_AON_1V8
  PDM0_CLK  -> MIC_CLK
  PDM0_D0   <- MIC_DATA_A
  PDM0_D1   <- MIC_DATA_B
  UART2     <-> U100 AR1 module low-speed control channel
  GPIO      <- BTN_CONFIRM, BTN_CANCEL, CHG_INT, FG_ALERT, HIRQ
```

### 推荐外设

```text
U220 SE050
  VCC -> +VDD_SENSOR_IO_1V8
  SDA/SCL -> I2C_AON_1V8
  RST/GPIO -> RT685 GPIO

U230 1.8 V QSPI NOR
  VCC -> +VDD_SENSOR_IO_1V8  or  +VDD_AUX_3V3 (二选一)
  IO0-IO3/CLK/CS -> RT685 FlexSPI
```

### 设计决策

- `SE050` 放在 RT685 一侧，而不是 AR1 一侧。这样眼镜的物理确认、设备身份证书、AP2 本地签名和 provisioning 都可以在 AON 域完成。
- `dawn_mcu` 里现有的“物理签名”思路可以直接迁移到 `RT685 + SE050 + 物理按钮`。
- 如果 QSPI Flash 最终使用 `3.3 V` 器件，则放到 `+VDD_AUX_3V3`；如果想简化电平域，优先改成 `1.8 V` NOR。

## 原理图页 3：传感器总线

### 传感器连接关系

```text
I2C_AON_1V8:
  U300 BHI360  (host side)
  U310 TMF8828
  U311 TMD3725
  U220 SE050
  U403 PCA9306 -> U402 MAX17048

U300 BHI360 AUX I2C:
  U301 BMM350
```

### BHI360 / BMM350

```text
U300 BHI360
  VDD    -> +VDD_SENSOR_IO_1V8
  VDDIO  -> +VDD_SENSOR_IO_1V8
  HIRQ   -> RT685 GPIO interrupt
  HSDO/HSCX/HCSB -> strap for I2C host mode
  ASDX/ASCX -> AUX I2C SDA/SCL

U301 BMM350
  VDD/VDDIO -> +VDD_SENSOR_IO_1V8
  SDA/SCL   -> BHI360 AUX I2C
  INT       -> optional to BHI360 / DNP
```

### TMF8828 / TMD3725

```text
U310 TMF8828
  VDD  -> +VDD_SENSOR_IO_1V8
  SDA/SCL -> I2C_AON_1V8
  INT  -> RT685 GPIO
  EN   -> RT685 GPIO (optional)

U311 TMD3725
  VDD -> +VDD_SENSOR_IO_1V8
  SDA/SCL -> I2C_AON_1V8
  INT -> RT685 GPIO
```

### 传感器布板要求

- `BHI360` 尽量靠近镜架几何中心，减少姿态偏移。
- `BMM350` 与扬声器磁路、振子、电池和高电流回路保持距离。
- `TMF8828` 与前向视场同轴，前盖玻璃与 air gap 按官方 kit 结构先做。
- `TMD3725` 靠近显示/目镜区域，用于亮度自适应与佩戴检测更合适。

## 原理图页 4：麦克风阵列与按键

### 麦克风阵列

建议第一版做 4 麦：

- `U320`: 左前
- `U321`: 右前
- `U322`: 左镜腿
- `U323`: 右镜腿

```text
All T5838:
  VDD -> +VDD_SENSOR_IO_1V8
  GND -> GND
  CLK -> MIC_CLK (shared)

Pair A:
  DATA -> MIC_DATA_A
  L/R strap opposite

Pair B:
  DATA -> MIC_DATA_B
  L/R strap opposite
```

### 人在环按钮

```text
SW230 CONFIRM
  one side -> GND
  other side -> RT685 GPIO with 100k pull-up to +VDD_AON_1V8

SW231 CANCEL
  one side -> GND
  other side -> RT685 GPIO with 100k pull-up to +VDD_AON_1V8

LED230 STATUS_LED
  +VDD_AUX_3V3 -> Rled -> LED -> RT685 GPIO sink
```

### 设计说明

- 这两个物理按钮不只是 UI，还是 Dawn 审批链和本地签名链的硬件根。
- `CONFIRM` 应该直接进入 RT685 的安全状态机，不经过 Android UI 才算安全。

## 原理图页 5：AR1 媒体链路

### 说明

这一页不能假装做成离散级可投板原理图。`AR1`、`JBD` 光机、`Sony ECX350F` 驱动板通常都需要 vendor reference design、FAE 文档或 NDA 资料。

所以第一版按“参考模块接口”处理。

### AR1 媒体接口草案

```text
U100 AR1 module
  VIN_MAIN   <- +SYS_MAIN
  UART_LP    <-> RT685 UART2
  I2C_CTRL   <-> RT685 optional / board management
  CSI0 4-lane -> U500 IMX662-AAQR1
  DSI0       -> J510 optical-engine connector
  I2S/PCM    -> optional audio amp / DNP in EVT1
  WLAN/BT    -> ANT100/ANT101
  USB2/UART  -> debug/programming header
```

### Camera path

```text
U500 IMX662-AAQR1
  AVDD   <- CAM_AVDD_3V3  (from AR1 reference power tree)
  DVDD   <- CAM_DVDD_1V1
  IOVDD  <- CAM_IOVDD_1V8
  XCLK   <- AR1 CAM_MCLK
  CSI-2  -> AR1 CSI0 (2-lane or 4-lane)
  I2C    -> AR1 CAM_I2C
  RESET/PWDN -> AR1 GPIO
```

### Display path

二选一装配：

```text
Option A:
  U510 JBD Hummingbird II driver / engine
    MIPI <- AR1 DSI0
    PWR  <- vendor reference rail
    OPT  -> Cellid Glass Waveguide G3

Option B:
  U511 Sony ECX350F driver board
    MIPI DSI <- AR1 DSI0
    PWR      <- vendor reference rail
    OPT      -> birdbath / waveguide bring-up optics
```

### 设计决策

- `Hummingbird II` 是主路线，因为它更贴近轻薄全彩 AR 眼镜。
- `ECX350F` 是 bring-up 备选路线，用来更快把 UI、亮度和菜单链路跑通。
- `Cellid Glass Waveguide G3` 是无源光学件，不在电原理图里给电气 pin。

## 关键信号定义

| Net | Source | Destination | Notes |
| --- | --- | --- | --- |
| `+SYS_MAIN` | BQ25619 SYS | AR1, PCA9420 | 主系统供电 |
| `+VDD_RT685_CORE_1V1` | PCA9420 SW1 | RT685 core | RT685 内核电源 |
| `+VDD_SENSOR_IO_1V8` | PCA9420 SW2 | RT685 IO, sensors, mics, SE050 | AON / sensor 主电源 |
| `+VDD_AUX_3V3` | PCA9420 LDO2 | status LED, optional flash, aux | 辅助 3.3 V 电源 |
| `I2C_AON_1V8` | RT685 | BHI360, TMF8828, TMD3725, SE050 | 2.2k pull-up 建议 |
| `BHI_AUX_I2C` | BHI360 | BMM350 | 由 BHI360 管理 |
| `MIC_CLK` | RT685 | 4x T5838 | 共享 PDM 时钟 |
| `MIC_DATA_A/B` | T5838 pairs | RT685 | 两组 PDM 数据 |
| `UART_AR1_LP` | RT685 | AR1 | 姿态、审批、低速控制 |
| `CSI0_CAM` | IMX662 | AR1 | 媒体主摄链路 |
| `DSI0_OPT` | AR1 | JBD / Sony driver | 近眼显示链路 |

## 建议的下一步

1. 先按本文件把 `Page 1 电源` 和 `Page 2 RT685 + 传感器` 落成真正的 KiCad 原理图。
2. 同时向 Qualcomm / Thundercomm、JBD、Sony、Cellid 索取参考设计，拿到 `AR1 + 光机` 页的真实 pin map。
3. 在 EVT1 阶段先允许 `AR1 + ECX350F` 或 `AR1 + vendor optical eval board`，不要等最终波导结构成熟后再 bring-up。
4. 把 `RT685 + SE050 + CONFIRM/CANCEL` 做成单独可测试子板，尽早把 Dawn 的物理确认链路接起来。

## 官方资料

- [Qualcomm AR1 product brief](https://docs.qualcomm.com/bundle/publicresource/87-86507-1_REV_B_Snapdragon_AR1_Gen_1_Platform_Product_Brief.pdf)
- [Thundercomm AI glasses reference design](https://www.thundercomm.com/zh/ai-glasses-mr-ces-2025/)
- [NXP RT685 EVK](https://www.nxp.com/design/development-boards/i-mx-evaluation-and-development-boards/i-mx-rt600-evaluation-kit%3AMIMXRT685-EVK)
- [NXP PCA9420/PCA9421](https://www.nxp.com/products/PCA9420-PCA9421)
- [NXP NX5P3090 datasheet](https://www.nxp.com/docs/en/data-sheet/NX5P3090.pdf)
- [NXP SE050](https://www.nxp.com/SE050)
- [Bosch BHI360](https://www.bosch-sensortec.com/en/products/smart-sensor-systems/bhi360)
- [Bosch BMM350 flyer](https://www.bosch-sensortec.com/media/boschsensortec/downloads/product_flyer/bst-bmm350-fl000.pdf)
- [ams TMF8828 evaluation kit](https://ams-osram.com/products/boards-kits-accessories/kits/ams-tmf8828-evm-eb-shield-evaluation-kit)
- [ams TMD3725](https://ams-osram.com/products/sensor-solutions/ambient-light-color-spectral-proximity-sensors/ams-tmd3725-color-sensor-module)
- [TDK T5838](https://invensense.tdk.com/products/digital/t5838/)
- [TI BQ25619](https://www.ti.com/product/BQ25619)
- [TI TPD4E05U06](https://www.ti.com/product/TPD4E05U06)
- [NXP PCA9306](https://www.nxp.com/products/PCA9306)
- [ADI MAX17048](https://www.analog.com/en/products/max17048.html)
- [Sony IMX662-AAQR1 flyer](https://www.sony-semicon.com/files/62/flyer_security/IMX662-AAQR_AAQR1_Flyer.pdf)
- [Sony ECX350F announcement](https://www.sony-semicon.com/en/news/2024/2024092401.html)
- [JBD Hummingbird II](https://www.jb-display.com/product_des/17.html)
- [Cellid Glass Waveguide G3 spec](https://cellid.com/wp-content/uploads/2024/01/Spec-Sheet-Glass-G3-Pla-Green-Pla-G1_v240101.pdf)
- [Dispelix Selvä evaluation unit](https://dispelix.com/uploads/images/Dispelix_brochure_evaluation-unit.pdf)
