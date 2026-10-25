"""Conservative vocal-register suggestions; timestamps and lyrics never change."""
import json
from pathlib import Path
import sys

import numpy as np


def _centers(values):
    centers=np.percentile(values,[25,75])
    for _ in range(30):
        low=values<np.mean(centers)
        if not low.any() or low.all():return None
        updated=np.array([np.mean(values[low]),np.mean(values[~low])])
        if np.max(np.abs(updated-centers))<.001:return updated
        centers=updated
    return centers


def _line_registers(times,pitches,valid,segments):
    """Use clear, recurring line registers when melodies fill the frame-level valley.

    Pitch cannot identify a person's gender or prove that two people are singing.
    These are editable low/high register suggestions; ambiguous lines stay untouched.
    """
    line_values=[]
    for segment in segments:
        region=(times>=segment['start'])&(times<=segment['end'])
        values=np.log2(pitches[region&valid])
        if len(values)<5 or len(values)<np.count_nonzero(region)*.5:
            values=np.array([])
        line_values.append(values)
    medians=np.array([np.median(values) for values in line_values if len(values)])
    if len(medians)<6:return None
    centers=_centers(medians)
    if centers is None or centers[1]-centers[0]<.5:return None
    mid=float(np.mean(centers))
    roles=[]
    for values in line_values:
        # Require most of a line to lie a semitone away from the boundary.
        # A line spanning both registers is not evidence of simultaneous singing.
        if not len(values):roles.append(None)
        elif np.mean(values<mid-1/12)>=.75:roles.append('male')
        elif np.mean(values>mid+1/12)>=.75:roles.append('female')
        else:roles.append(None)
    if min(roles.count('male'),roles.count('female'))<3:return None
    ordered=[roles[i] for i in sorted(range(len(segments)),key=lambda i:segments[i]['start']) if roles[i]]
    # Do not split a single rising/falling passage into two supposed singers.
    if sum(a!=b for a,b in zip(ordered,ordered[1:]))<2:return None
    return roles,centers


def suggest(times, pitches, segments):
    times=np.asarray(times);pitches=np.asarray(pitches)
    in_lyrics=np.zeros(times.shape,dtype=bool)
    for segment in segments:
        in_lyrics|=(times>=segment['start'])&(times<=segment['end'])
    valid=np.isfinite(pitches)&(pitches>=65)&(pitches<=650)
    values=np.log2(pitches[valid&in_lyrics])
    result={'roles':[None]*len(segments),'two_registers':False,'method':'pyin-registers-v2',
            'male':0,'female':0,'shared':0,'uncertain':len(segments),
            'reason':'insufficient_voiced_audio','confidence':'unconfirmed'}
    if len(values)<30:return result
    result['reason']='overlapping_registers'
    centers=_centers(values)
    if centers is None:return result
    mid=float(np.mean(centers));gap=float(centers[1]-centers[0])
    mass=float(np.mean(values<mid))
    valley=float(np.mean(np.abs(values-mid)<max(.03,gap*.12)))
    peaks=max(float(np.mean(np.abs(values-c)<max(.03,gap*.12))) for c in centers)
    # A continuous melody spanning low/high notes is insufficient evidence of two voices.
    if gap<.45 or min(mass,1-mass)<.18 or valley>.15 or valley>peaks*.65:
        by_line=_line_registers(times,pitches,valid,segments)
        if by_line is None:return result
        result['roles'],centers=by_line
        mid=float(np.mean(centers));gap=float(centers[1]-centers[0])
        result.update(method='pyin-line-registers-v2',reason='recurring_line_registers')
    else:
        result['reason']='separated_frame_registers'
        for i,segment in enumerate(segments):
            mask=valid&(times>=segment['start'])&(times<=segment['end'])
            frequencies=pitches[mask]
            if len(frequencies)<5:continue
            low=float(np.mean(frequencies<2**mid))
            role='all' if .3<=low<=.7 else 'male' if low>.7 else 'female'
            result['roles'][i]=role
    result.update(two_registers=True,split_hz=round(2**mid,1),separation_semitones=round(gap*12,1),
                  confidence='register_suggestion')
    for role,key in [('male','male'),('female','female'),('all','shared')]:
        result[key]=result['roles'].count(role)
    result['uncertain']=result['roles'].count(None)
    return result


def analyze(vocal,segments,progress=lambda *_:None):
    source=Path(vocal)
    stat=source.stat()
    signature=f'pyin-v1:{stat.st_size}:{stat.st_mtime_ns}'
    cache=source.with_name('duet_pitch.npz')
    try:
        with np.load(cache,allow_pickle=False) as stored:
            if str(stored['signature'])!=signature:raise ValueError('stale pitch cache')
            times,pitches=stored['times'],stored['pitches']
        progress(94,'Đang phân vai từ cao độ đã phân tích…')
    except (OSError,ValueError,KeyError):
        import librosa
        sr=16000;hop=1024;chunk=20*sr
        progress(44,'Đang chuẩn bị phân tích giọng hát…')
        audio,_=librosa.load(source,sr=sr,mono=True)
        parts=[];positions=[]
        for offset in range(0,len(audio),chunk):
            end=min(len(audio),offset+chunk)
            # Context avoids an artificial pitch boundary at each chunk edge.
            start=max(0,offset-2048);stop=min(len(audio),end+2048)
            data=audio[start:stop]
            if len(data)<2048:continue
            f0,voiced,_=librosa.pyin(data,fmin=65,fmax=650,sr=sr,frame_length=2048,hop_length=hop,resolution=.25)
            ts=start/sr+librosa.times_like(f0,sr=sr,hop_length=hop)
            keep=(ts>=offset/sr)&(ts<end/sr)
            parts.append(np.where(voiced[keep],f0[keep],np.nan));positions.append(ts[keep])
            progress(45+48*end/max(1,len(audio)),'Đang nghe vocal để gợi ý Nam / Nữ…')
        times=np.concatenate(positions) if positions else np.array([])
        pitches=np.concatenate(parts) if parts else np.array([])
        temporary=cache.with_name('duet_pitch_next.npz')
        np.savez_compressed(temporary,times=times,pitches=pitches,signature=np.array(signature))
        temporary.replace(cache)
    return suggest(times,pitches,segments)


if __name__=='__main__':
    request=json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
    def progress(percent,message):
        print('OBS_PROGRESS '+json.dumps({'progress':percent,'message':message},ensure_ascii=False),flush=True)
    result=analyze(request['vocal'],request['segments'],progress)
    Path(request['output']).write_text(json.dumps(result,ensure_ascii=False),encoding='utf-8')
