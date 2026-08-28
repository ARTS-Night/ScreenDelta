#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub size: Size,
}

impl Region {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        (width > 0 && height > 0).then_some(Self {
            x,
            y,
            size: Size { width, height },
        })
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right =
            (self.x as i64 + self.size.width as i64).min(other.x as i64 + other.size.width as i64);
        let bottom = (self.y as i64 + self.size.height as i64)
            .min(other.y as i64 + other.size.height as i64);
        if right <= left as i64 || bottom <= top as i64 {
            return None;
        }
        Self::new(
            left,
            top,
            (right - left as i64) as u32,
            (bottom - top as i64) as u32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Region;
    #[test]
    fn intersection_uses_physical_desktop_coordinates() {
        assert_eq!(
            Region::new(-100, 0, 200, 100)
                .unwrap()
                .intersection(Region::new(0, 50, 100, 100).unwrap()),
            Region::new(0, 50, 100, 50)
        );
    }
}
