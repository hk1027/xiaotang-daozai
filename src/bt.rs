//! 经典蓝牙与低功耗蓝牙设备的连接状态和电量。

use std::time::{Duration,Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub battery: Option<u8>,
    pub kind: DeviceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind { Bone, Earbuds, Headphones, Speaker, Keyboard, Mouse, Gamepad, Phone, Other }

fn device_kind(name: &str, classic: bool, class: Option<u32>) -> DeviceKind {
    use DeviceKind::*;
    let name = name.to_lowercase();
    if ["shokz", "aftershokz", "openmove", "openrun", "骨传导"].iter().any(|s| name.contains(s)) { return Bone; }
    if ["earbuds", "airpods", "buds", "earphones", "入耳"].iter().any(|s| name.contains(s)) { return Earbuds; }
    if ["keyboard", "键盘"].iter().any(|s| name.contains(s)) { return Keyboard; }
    if ["mouse", "鼠标"].iter().any(|s| name.contains(s)) { return Mouse; }
    if ["gamepad", "controller", "手柄"].iter().any(|s| name.contains(s)) { return Gamepad; }
    if ["speaker", "音箱", "音响"].iter().any(|s| name.contains(s)) { return Speaker; }
    if let Some(class) = class {
        if classic {
            match (class >> 8) & 0x1f {
                2 => return Phone,
                4 => return if matches!((class >> 2) & 0x3f, 5 | 7 | 10) { Speaker } else { Headphones },
                5 => return match (class >> 6) & 3 { 1 => Keyboard, 2 => Mouse, 3 => Keyboard, _ => Other },
                _ => {}
            }
        } else {
            match (class >> 6, class & 0x3f) {
                (15, 1) => return Keyboard,
                (15, 2) => return Mouse,
                (15, 3 | 4) => return Gamepad,
                (1, _) => return Phone,
                _ => {}
            }
        }
    }
    if ["headphone", "headset", "耳机"].iter().any(|s| name.contains(s)) { Headphones } else { Other }
}

impl Device {
    /// 通知条正文：`名称 · 电量 80%`，设备没上报电量时只写名称。
    pub fn summary(&self) -> String {
        match self.battery {
            Some(b) => format!("{} · 电量 {b}%", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Default)]
struct Cache {devices:Vec<Device>,battery_at:Option<Instant>}
impl Cache {
    fn update(&mut self,mut devices:Vec<Device>,now:Instant,read:impl FnOnce(&[Device])->Result<Vec<(String,u8)>,()>)->Vec<Device>{
        let changed=devices.len()!=self.devices.len() || devices.iter().any(|d|!self.devices.iter().any(|old|old.id==d.id));
        for device in &mut devices {device.battery=self.devices.iter().find(|old|old.id==device.id).and_then(|old|old.battery);}
        if devices.is_empty(){self.battery_at=None;}
        else if changed || self.battery_at.is_none_or(|at|now.duration_since(at)>=Duration::from_secs(60)) {
            // 失败同样限频，避免设备驱动异常时每两秒重新枚举整机。
            self.battery_at=Some(now);
            if let Ok(levels)=read(&devices){for device in &mut devices {device.battery=levels.iter().find(|(id,_)|id==&device.id).map(|(_,level)|*level);}}
        }
        self.devices=devices.clone();devices
    }
}

/// 释放服务时唤醒后台线程，停止后续扫描。
pub struct Service {
    pub rx: std::sync::mpsc::Receiver<Vec<Device>>,
    stop: std::sync::mpsc::Sender<()>,
}
impl Drop for Service {
    fn drop(&mut self) {let _=self.stop.send(());}
}

pub fn spawn() -> Service {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let (stop, stopped)=std::sync::mpsc::channel();
    std::thread::Builder::new().name("daozai-bluetooth".into()).spawn(move || {
        #[cfg(windows)]
        let mut cache=Cache::default();
        loop {
            if stopped.try_recv()!=Err(std::sync::mpsc::TryRecvError::Empty) {return;}
            #[cfg(windows)]
            if let Ok(found)=query(&mut cache) {
                if matches!(tx.try_send(found),Err(std::sync::mpsc::TrySendError::Disconnected(_))) {return;}
            }
            #[cfg(not(windows))]
            let _=&tx;
            if stopped.recv_timeout(Duration::from_secs(2))!=Err(std::sync::mpsc::RecvTimeoutError::Timeout) {return;}
        }
    }).ok();
    Service {rx,stop}
}

#[cfg(windows)]
fn query(cache:&mut Cache) -> Result<Vec<Device>, ()> {
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
    use windows::Devices::Enumeration::DeviceInformation;
    let selectors = [
        (true, BluetoothDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected).map_err(|_| ())?),
        (false, BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected).map_err(|_| ())?),
    ];
    let mut devices = Vec::new();
    for (classic, selector) in selectors {
        let selector = windows::core::HSTRING::from(format!("({selector}) AND System.Devices.Aep.IsConnected:=System.StructuredQueryType.Boolean#True"));
        let found = DeviceInformation::FindAllAsyncAqsFilter(&selector).map_err(|_| ())?.get().map_err(|_| ())?;
        for info in found {
            let Ok(id) = info.Id() else { continue };
            let Ok(name) = info.Name() else { continue };
            let name = name.to_string();
            if name.trim().is_empty() { continue; }
            let address = id.to_string().rsplit('-').next().unwrap_or_default().replace(':', "").to_ascii_uppercase();
            if devices.iter().any(|d: &Device| d.id == address) { continue; }
            let kind=if let Some(old)=cache.devices.iter().find(|old|old.id==address && old.name==name){old.kind}else{
                let class=if classic {
                    BluetoothDevice::FromIdAsync(&id).ok().and_then(|a|a.get().ok()).and_then(|d|{
                        let class=d.ClassOfDevice().ok().and_then(|c|c.RawValue().ok());let _=d.Close();class
                    })
                }else{
                    BluetoothLEDevice::FromIdAsync(&id).ok().and_then(|a|a.get().ok()).and_then(|d|{
                        let class=d.Appearance().ok().and_then(|c|c.RawValue().ok()).map(u32::from);let _=d.Close();class
                    })
                };
                device_kind(&name,classic,class)
            };
            devices.push(Device { id: address, kind, name, battery: None });
        }
    }
    devices.sort_by(|a,b|a.id.cmp(&b.id));
    Ok(cache.update(devices,Instant::now(),read_batteries))
}

#[cfg(windows)]
fn read_batteries(devices:&[Device])->Result<Vec<(String,u8)>,()> {
    use windows::core::{GUID, PCWSTR};
    use windows::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows::Win32::Devices::Properties::DEVPROPKEY;
    use windows::Win32::Foundation::HWND;

    // DEVPKEY_Bluetooth_Battery {104EA319-6EE2-4701-BD47-8DDBF425BBE5}, 2
    const BATTERY: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
        pid: 2,
    };
    let mut batteries: Vec<(String, u8)> = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(None, PCWSTR::null(), HWND::default(), DIGCF_PRESENT | DIGCF_ALLCLASSES)
            .map_err(|_| ())?;
        for i in 0..u32::MAX {
            let mut data = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInfo(set, i, &mut data).is_err() {
                break;
            }
            let Some(instance) = instance_id(set, &data) else { continue };
            let upper = instance.to_ascii_uppercase();
            if !devices.iter().any(|device|upper.contains(&device.id)){continue;}
            if let Some(level) = byte_prop(set, &data, &BATTERY).filter(|b| *b <= 100) {
                batteries.push((upper.clone(), level));
            }
        }
        // 每次枚举后释放设备列表句柄。
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    Ok(devices.iter().filter_map(|device|batteries.iter().find(|(node,_)|node.contains(&device.id)).map(|(_,level)|(device.id.clone(),*level))).collect())
}

#[cfg(windows)]
unsafe fn instance_id(
    set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDeviceInstanceIdW;
    let mut buf = [0u16; 512];
    let mut need = 0u32;
    SetupDiGetDeviceInstanceIdW(set, data, Some(&mut buf), Some(&mut need)).ok()?;
    Some(String::from_utf16_lossy(&buf[..(need as usize).saturating_sub(1)]))
}

#[cfg(windows)]
unsafe fn byte_prop(
    set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
    key: &windows::Win32::Devices::Properties::DEVPROPKEY,
) -> Option<u8> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW;
    use windows::Win32::Devices::Properties::DEVPROPTYPE;
    // DEVPROP_TYPE_BYTE
    const BYTE: DEVPROPTYPE = DEVPROPTYPE(0x3);
    let mut ty = DEVPROPTYPE::default();
    let mut buf = [0u8; 1];
    let mut size = 0u32;
    SetupDiGetDevicePropertyW(set, data, key, &mut ty, Some(&mut buf), Some(&mut size), 0).ok()?;
    (ty == BYTE && size == 1).then_some(buf[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id:&str)->Device {Device{id:id.into(),name:id.into(),battery:None,kind:DeviceKind::Mouse}}

    #[test]
    fn idle_bluetooth_does_not_rescan_all_hardware_every_two_seconds(){
        let start=std::time::Instant::now();let mut cache=Cache::default();let mut scans=0;
        for second in (0..360).step_by(2){
            let found=cache.update(vec![device("AA")],start+Duration::from_secs(second),|_|{scans+=1;Ok(vec![("AA".into(),80)])});
            assert_eq!(found[0].battery,Some(80));
        }
        assert_eq!(scans,6,"六分钟内同一设备的电量扫描应最多每分钟一次");
    }

    #[test]
    fn bluetooth_connection_change_refreshes_immediately_and_failed_scans_back_off(){
        let now=std::time::Instant::now();let mut cache=Cache::default();
        cache.update(vec![device("AA")],now,|_|Ok(vec![("AA".into(),80)]));
        let found=cache.update(vec![device("AA"),device("BB")],now+Duration::from_secs(2),|_|Ok(vec![("AA".into(),79),("BB".into(),60)]));
        assert_eq!(found[1].battery,Some(60));
        let found=cache.update(vec![device("AA"),device("BB")],now+Duration::from_secs(62),|_|Err(()));
        assert_eq!(found[0].battery,Some(79));
        cache.update(vec![device("AA"),device("BB")],now+Duration::from_secs(64),|_|panic!("失败后不能立即重复扫描"));
        assert!(cache.update(vec![],now+Duration::from_secs(66),|_|panic!("无设备不能扫描整机")).is_empty());
        cache.update(vec![device("AA")],now+Duration::from_secs(68),|_|Ok(vec![("AA".into(),78)]));
        assert_eq!(cache.devices[0].battery,Some(78));
    }

    #[test]
    fn identifies_connected_audio_and_input_devices() {
        assert_eq!(device_kind("OpenMove by Shokz", true, Some(0x240418)), DeviceKind::Bone);
        assert_eq!(device_kind("QingLong 87 Ultra 8K", false, Some(961)), DeviceKind::Keyboard);
        assert_eq!(device_kind("MX Anywhere", false, Some(962)), DeviceKind::Mouse);
        assert_eq!(device_kind("耳机", true, Some(0x0404)), DeviceKind::Headphones);
        assert_eq!(device_kind("AirPods", true, None), DeviceKind::Earbuds);
        assert_eq!(device_kind("未知设备", false, None), DeviceKind::Other);
    }

    #[test]
    fn summary_omits_missing_battery() {
        let d = Device { id: "1".into(), name: "Headset".into(), battery: None, kind: DeviceKind::Headphones };
        assert_eq!(d.summary(), "Headset");
        let d = Device { battery: Some(80), ..d };
        assert_eq!(d.summary(), "Headset · 电量 80%");
    }
}

#[cfg(test)]
mod lifecycle_tests {
    #[test]
    fn dropping_service_signals_stop_even_without_successful_scans(){
        let (_tx,rx)=std::sync::mpsc::channel();
        let (stop,stopped)=std::sync::mpsc::channel();
        drop(super::Service {rx,stop});
        assert_eq!(stopped.recv_timeout(std::time::Duration::from_millis(100)),Ok(()));
    }
}
