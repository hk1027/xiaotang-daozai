//! OLED 像素偏移：岛体收起静止时沿停靠边缓慢移动几个像素，
//! 让常驻的时间、电量等内容不在同一块像素上长期停留。
//!
//! 目的是**减轻** OLED 静态显示留下的痕迹，不承诺阻止烧屏；默认关闭，
//! 打开后也不改动用户保存的位置（`island_offset` 不受影响）。
//!
//! 只在收起、可见且没有交互时移动；展开、点击、拖拽时保持当前位置。
//! 移动只沿不贴边的那一轴进行，贴边的一侧保持严丝合缝。
use std::time::Duration;

use crate::model::Dock;

/// 单方向最大位移，物理像素。
pub const MAX_SHIFT: i32 = 3;
/// 每一步的间隔。
pub const STEP: Duration = Duration::from_secs(45);
/// 一个来回的步数：0 → MAX_SHIFT → -MAX_SHIFT → 0。
pub const CYCLE: u32 = MAX_SHIFT as u32 * 4;

/// 时间表上的位移：0 → 1 → … → MAX_SHIFT → … → -MAX_SHIFT → … → 0。
///
/// 每一步只差 1 个像素，所以位置不会突然跳变；一个来回 `CYCLE` 步。
pub fn offset(elapsed: Duration) -> i32 {
    let max = MAX_SHIFT as i64;
    if max <= 0 { return 0; }
    let period = max * 4;
    let step = (elapsed.as_millis() / STEP.as_millis()) as i64;
    let phase = step.rem_euclid(period);
    if phase <= max {
        phase as i32
    } else if phase <= max * 3 {
        (max * 2 - phase) as i32
    } else {
        (phase - period) as i32
    }
}

/// 当前应该应用的位移（物理像素）；`enabled` 为 false 时恒为 `(0, 0)`。
///
/// 顶部停靠只左右移动，左右停靠只上下移动：动了贴边那一侧，刘海和屏幕边缘之间会裂开。
pub fn shift(enabled: bool, dock: Dock, elapsed: Duration) -> (i32, i32) {
    if !enabled { return (0, 0); }
    let px = offset(elapsed);
    if dock.is_side() { (0, px) } else { (px, 0) }
}

/// 向目标位移靠近，单轴每次最多 1 个像素。
///
/// 展开、拖拽期间位置保持不变，长时间交互后目标可能已经走到别处；
/// 逐像素收敛可以避免整块面板（连同展开动画）一次跳好几个像素。
pub fn toward(current: (i32, i32), target: (i32, i32)) -> (i32, i32) {
    fn step(from: i32, to: i32) -> i32 { from + (to - from).clamp(-1, 1) }
    (step(current.0, target.0), step(current.1, target.1))
}

/// 是否该移动：收起、没有拖拽、不在编辑。
///
/// 展开、点击、拖拽期间保持当前位置；回到收起静止后再继续。
pub fn should_move(expanded: bool, dragging: bool, editing: bool) -> bool {
    !expanded && !dragging && !editing
}

/// 距离下一次位移还有多久。
///
/// 主窗口把它交给现有的重绘调度（`request_repaint_after`），到点醒一次即可，
/// 不额外起线程、不做高频轮询。刚跨过一步时返回整整一个间隔，不会原地空转。
pub fn until_next_step(elapsed: Duration) -> Duration {
    let step = STEP.as_millis();
    let left = (step - elapsed.as_millis() % step) as u64;
    Duration::from_millis(left.clamp(150, step as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_moves_one_pixel_and_the_walk_covers_both_sides() {
        let period = CYCLE;
        let mut previous = offset(Duration::ZERO);
        let (mut lowest, mut highest) = (previous, previous);
        assert_eq!(previous, 0, "启动时先贴边，不改变默认位置");
        for step in 1..=period * 2 {
            let current = offset(STEP * step);
            assert!((current - previous).abs() == 1, "第 {step} 步只应移动 1 个像素：{previous} → {current}");
            (lowest, highest) = (lowest.min(current), highest.max(current));
            previous = current;
        }
        assert_eq!((lowest, highest), (-MAX_SHIFT, MAX_SHIFT), "一个来回应两侧都用满，再回到贴边位置");
        assert_eq!(offset(STEP * period), 0);
    }

    #[test]
    fn shift_follows_the_free_axis_and_respects_the_switch() {
        let elapsed = STEP * MAX_SHIFT as u32; // 走到位移上限
        for dock in [Dock::Top, Dock::Left, Dock::Right] {
            let side = dock.is_side();
            let (x, y) = shift(true, dock, elapsed);
            assert_eq!((x, y), if side {(0, MAX_SHIFT)} else {(MAX_SHIFT, 0)}, "{dock:?}：只能沿不贴边的一轴移动");
            assert_eq!(shift(false, dock, elapsed), (0, 0), "{dock:?}：关闭时必须回到贴边位置");
        }
    }

    #[test]
    fn position_holds_within_a_step_and_starts_over_after_one_cycle() {
        let within = STEP * 2u32 + Duration::from_secs(44);
        assert_eq!(offset(within), offset(STEP * 2u32), "同一步内位置不变，避免窗口抖动");
        let period = STEP * CYCLE;
        for step in [0u32, 1, 5, 11] {
            assert_eq!(offset(STEP * step), offset(period + STEP * step), "一个来回后重复同样的位移");
        }
        assert!(offset(Duration::MAX).abs() <= MAX_SHIFT, "超长运行时间不应溢出");
    }

    #[test]
    fn the_next_step_wakes_up_exactly_on_the_boundary() {
        let step = STEP.as_millis() as u64;
        assert_eq!(until_next_step(Duration::ZERO), STEP, "刚挪完就约下一个间隔，不空转");
        assert_eq!(until_next_step(Duration::from_millis(1)), Duration::from_millis(step - 1));
        assert_eq!(until_next_step(STEP * 2u32 + Duration::from_secs(1)), Duration::from_secs(44));
        assert_eq!(until_next_step(STEP * 2u32), STEP);
        for left in [0u64, 1, 100, 149] {
            let wait = until_next_step(STEP * 3u32 + Duration::from_millis(step - left));
            assert!(wait >= Duration::from_millis(150) && wait <= STEP, "刚过边界时也要留出最小间隔：{left} → {wait:?}");
        }
    }

    #[test]
    fn expanded_dragging_or_editing_holds_the_position() {
        assert!(should_move(false, false, false), "收起静止时应该移动");
        assert!(!should_move(true, false, false), "展开时保持当前位置");
        assert!(!should_move(false, true, false), "拖拽时保持当前位置");
        assert!(!should_move(false, false, true), "编辑时保持当前位置");
        assert!(!should_move(true, true, true));
    }

    #[test]
    fn reopening_walks_back_one_pixel_at_a_time() {
        let target = shift(false, Dock::Top, Duration::ZERO);
        assert_eq!(target, (0, 0));
        // 展开前的位移是 3，直接恢复目标会让整块面板跳 3 像素。
        let mut current = (3, 0);
        let mut frames = 0;
        while current != (0, 0) && frames < 10 {
            current = toward(current, (0, 0));
            frames += 1;
            assert!(frames <= 3, "最多 3 帧就应该收敛");
        }
        assert_eq!(current, (0, 0));
        assert_eq!(toward((0, 0), (0, 0)), (0, 0), "已经到位时不动");
        assert_eq!(toward((3, 0), (-2, 0)), (2, 0), "单轴每次只走 1 个像素");
    }
}
