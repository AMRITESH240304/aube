use std::sync::OnceLock;

use tract_onnx::prelude::*;

pub const RATE: usize = 16_000;
const MIN_CONFIDENCE: f32 = 0.5;
const LOGITS: &str = "/clf/out/w/Add";

static MODEL: &[u8] = include_bytes!("../models/lid-ecapa-voxlingua107.int8.onnx");
static LABELS: &str = include_str!("../models/lid-labels.txt");

pub struct Detector {
    model: Arc<TypedRunnableModel>,
    labels: Vec<&'static str>,
}

pub fn detector() -> Option<&'static Detector> {
    static DETECTOR: OnceLock<Option<Detector>> = OnceLock::new();
    DETECTOR
        .get_or_init(|| {
            Detector::load()
                .inspect_err(|e| eprintln!("language detection unavailable: {e}"))
                .ok()
        })
        .as_ref()
}

impl Detector {
    fn load() -> TractResult<Self> {
        let mut model = tract_onnx::onnx().model_for_read(&mut &MODEL[..])?;
        let logits = model.node_by_name(LOGITS)?.id;
        model.select_output_outlets(&[OutletId::new(logits, 0)])?;
        let samples = model.symbols.sym("samples");
        let model = model
            .with_input_fact(0, f32::fact([TDim::from(1), samples.into()]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self {
            model,
            labels: LABELS.lines().collect(),
        })
    }

    pub fn detect(&self, speech: &[f32], candidates: &[&str]) -> Option<&'static str> {
        let input =
            tract_ndarray::Array2::from_shape_vec((1, speech.len()), speech.to_vec()).ok()?;
        let output = self.model.run(tvec!(input.into_tensor().into())).ok()?;
        let scores = output[0].to_plain_array_view::<f32>().ok()?;
        let allowed: Vec<(&'static str, f32)> = self
            .labels
            .iter()
            .zip(scores.iter())
            .filter(|(label, _)| candidates.contains(label))
            .map(|(label, score)| (*label, *score))
            .collect();
        let (best, top) = allowed.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1))?;
        let total: f32 = allowed.iter().map(|(_, score)| (score - top).exp()).sum();
        (1.0 / total >= MIN_CONFIDENCE).then_some(best)
    }
}

pub fn downsample(audio: &[f32], from: usize) -> impl Iterator<Item = f32> + '_ {
    let factor = from / RATE;
    audio
        .chunks_exact(factor)
        .map(move |frame| frame.iter().sum::<f32>() / factor as f32)
}
