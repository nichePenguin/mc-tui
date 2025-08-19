use std::ops::{Add, Sub};

pub fn pos_add<T>(
    a: (T, T, T),
    b: (T, T, T))
    -> (T, T, T)
    where T: Add<Output = T>
{
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

pub fn pos_sub<T>(
    a: (T, T, T),
    b: (T, T, T))
    -> (T, T, T)
    where T: Sub<Output = T>
{
    (a.0 - b.0, a.1 - b.1, a.2 - b.2)
}

pub fn world_pos(pos: (f64, f64, f64)) -> (i32, i32, i32) {
    ((pos.0 - 0.5).round() as i32,
    (pos.1) as i32,
    (pos.2 - 0.5).round() as i32)
}

pub fn in_square(
    point: (i32, i32, i32),
    relative: (i32, i32, i32),
    radius: i32,
    height: i32)
    -> bool
{
    let point = (point.0 - relative.0, point.1 - relative.1, point.2 - relative.2);
    point.0 > -radius && point.0 < radius && point.2 > -radius && point.2 < radius && point.1 > -height && point.1 < height
}

// Minecraft specific representation of fractional position as an integer
pub fn from_abs_int<T>(pos: (T, T, T)) -> (f64, f64, f64) 
    where T: Into<f64>
{
    (pos.0.into() / 32., pos.1.into() / 32., pos.2.into() / 32.,)
}
