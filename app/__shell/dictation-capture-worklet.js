// issue-407: live dictation capture, loaded by dictation.js with
// audioContext.audioWorklet.addModule(). Collects the mic's first channel
// into 4096-sample blocks and posts each block to the loop, which resamples
// to 16 kHz and feeds the sliding window. Produces no output.
class BramVoiceCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.block = new Float32Array(4096);
    this.fill = 0;
  }

  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (channel) {
      let i = 0;
      while (i < channel.length) {
        const n = Math.min(channel.length - i, this.block.length - this.fill);
        this.block.set(channel.subarray(i, i + n), this.fill);
        this.fill += n;
        i += n;
        if (this.fill === this.block.length) {
          this.port.postMessage(this.block);
          this.block = new Float32Array(4096);
          this.fill = 0;
        }
      }
    }
    return true;
  }
}

registerProcessor("bram-voice-capture", BramVoiceCapture);
