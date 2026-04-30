/**
 * LiquidGlassFilter — defines the SVG filters used by the
 * `.liquid-glass*` class family for true displacement-based refraction.
 *
 * Three filters are exposed:
 *
 *   #liquid-refract        scale 6   subtle, default-on for glass cards
 *   #liquid-refract-strong scale 18  hero surfaces, hover bursts
 *   #liquid-chromatic      scale 6   adds RGB channel separation at rim
 *
 * Mount once at the app root. Filters are reusable by id.
 */
export function LiquidGlassFilter() {
  return (
    <svg
      aria-hidden="true"
      focusable="false"
      style={{
        position: "fixed",
        top: 0,
        left: 0,
        width: 0,
        height: 0,
        pointerEvents: "none",
      }}
    >
      <defs>
        {/* Subtle, perf-conscious refraction — applied by default */}
        <filter
          id="liquid-refract"
          x="-12%"
          y="-12%"
          width="124%"
          height="124%"
          colorInterpolationFilters="sRGB"
        >
          <feTurbulence
            type="fractalNoise"
            baseFrequency="0.006 0.009"
            numOctaves="2"
            seed="3"
            result="turbulence"
          />
          <feGaussianBlur in="turbulence" stdDeviation="2" result="softNoise" />
          <feDisplacementMap
            in="SourceGraphic"
            in2="softNoise"
            scale="6"
            xChannelSelector="R"
            yChannelSelector="G"
          />
        </filter>

        {/* Stronger — hero surfaces, on hover/active bursts */}
        <filter
          id="liquid-refract-strong"
          x="-15%"
          y="-15%"
          width="130%"
          height="130%"
          colorInterpolationFilters="sRGB"
        >
          <feTurbulence
            type="fractalNoise"
            baseFrequency="0.011 0.014"
            numOctaves="2"
            seed="7"
            result="turbulence"
          />
          <feGaussianBlur in="turbulence" stdDeviation="2.5" result="softNoise" />
          <feDisplacementMap
            in="SourceGraphic"
            in2="softNoise"
            scale="18"
            xChannelSelector="R"
            yChannelSelector="G"
          />
        </filter>

        {/* Chromatic — separates R and B channels for rim dispersion.
            Splits the source into shifted R and B versions, then merges.
            This is the optical signature of real curved glass. */}
        <filter
          id="liquid-chromatic"
          x="-12%"
          y="-12%"
          width="124%"
          height="124%"
          colorInterpolationFilters="sRGB"
        >
          <feTurbulence
            type="fractalNoise"
            baseFrequency="0.006 0.009"
            numOctaves="2"
            seed="3"
            result="turbulence"
          />
          <feGaussianBlur in="turbulence" stdDeviation="2" result="softNoise" />

          {/* Red channel — displaced one direction */}
          <feDisplacementMap
            in="SourceGraphic"
            in2="softNoise"
            scale="8"
            xChannelSelector="R"
            yChannelSelector="G"
            result="rDisp"
          />
          <feColorMatrix
            in="rDisp"
            type="matrix"
            values="1 0 0 0 0
                    0 0 0 0 0
                    0 0 0 0 0
                    0 0 0 1 0"
            result="rOnly"
          />

          {/* Blue channel — displaced opposite direction */}
          <feDisplacementMap
            in="SourceGraphic"
            in2="softNoise"
            scale="-8"
            xChannelSelector="R"
            yChannelSelector="G"
            result="bDisp"
          />
          <feColorMatrix
            in="bDisp"
            type="matrix"
            values="0 0 0 0 0
                    0 0 0 0 0
                    0 0 1 0 0
                    0 0 0 1 0"
            result="bOnly"
          />

          {/* Green channel — undisplaced anchor */}
          <feColorMatrix
            in="SourceGraphic"
            type="matrix"
            values="0 0 0 0 0
                    0 1 0 0 0
                    0 0 0 0 0
                    0 0 0 1 0"
            result="gOnly"
          />

          {/* Merge all three back together */}
          <feBlend in="rOnly" in2="gOnly" mode="screen" result="rg" />
          <feBlend in="rg" in2="bOnly" mode="screen" />
        </filter>
      </defs>
    </svg>
  );
}
