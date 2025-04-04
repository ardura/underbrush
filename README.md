# Underbrush
![image](https://github.com/user-attachments/assets/b268d307-cd6e-436b-a548-e04a6685c222)

Underbrush is meant to be a subtle (or less subtle depending on use) console processing effect combining a few DSP things I liked.
Here is the process flow with some descriptions:

1) Input signal gets scaled by drive parameter
2) Saturation gets applied (depending on setting)
   - Tape: Soft saturation with smooth knee
   - Tube: Asymmetric Saturation
   - Transistor: Harder clipping with some curve
   - LDR: Light Dependent Resistor - The harder you drive it, the less resistance
   - Bypass: No saturation applied
4) Small amount of Stereo crosstalk added
5) DC Blocking happens
6) Phase linearization of low frequencies
   - This is set to 150hz
8) Slew limiter gets applied (if value < 1.0)
   - This is your vintage sound adder. Not as noticable on its own, but try to A/B and find a setting you like
   - It tends to roll off the highs and saturate things lightly at the same time
10) Auto compression happens (if enabled)
12) Output gain applied
13) Hard limiting applied at 0db (if enabled)

# Thanks
