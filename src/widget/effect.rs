use tachyonfx::{Effect, Shader};

#[allow(clippy::module_name_repetitions)]
pub trait EffectExt {
    fn reset(&mut self);
    fn running(&mut self) -> impl Iterator<Item = &mut Effect>;
}

impl EffectExt for Vec<Effect> {
    fn reset(&mut self) {
        self.iter_mut().for_each(Shader::reset);
    }

    fn running(&mut self) -> impl Iterator<Item = &mut Effect> {
        self.iter_mut().filter(|effect| effect.running())
    }
}
